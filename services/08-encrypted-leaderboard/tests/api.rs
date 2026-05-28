//! End-to-end tests des Leaderboard-Services.
//!
//! Empfohlener Aufruf:
//!   cargo test --release -p encrypted-leaderboard --test api -- --test-threads=1
//!
//! - `--release` ist wichtig: FHE-Operationen sind im Debug-Build ~10× langsamer.
//! - `--test-threads=1` verhindert dass mehrere Tests gleichzeitig einen
//!   ServerKey dekomprimieren (jede Decompress-Allokation ist mehrere hundert MB).

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use encrypted_leaderboard::{app, state::AppState};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tfhe::prelude::*;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheBool, FheUint16, FheUint8};
use tower::util::ServiceExt;

// ---------------------------------------------------------------------------
// Test-Helfer
// ---------------------------------------------------------------------------

// Ein TFHE-Schlüsselpaar wird genau einmal pro Test-Prozess erzeugt (~30–60 s).
// Tests teilen sich die teuren Bytes — jede Session dekomprimiert dann selbst.
struct TestKeys {
    client_key: ClientKey,
    server_key_b64: String,
}

fn keys() -> &'static TestKeys {
    static KEYS: OnceLock<TestKeys> = OnceLock::new();
    KEYS.get_or_init(|| {
        let config = ConfigBuilder::default().build();
        let client_key = ClientKey::generate(config);
        let compressed = CompressedServerKey::new(&client_key);
        let server_bytes = bincode::serialize(&compressed).expect("serialize server key");
        TestKeys {
            client_key,
            server_key_b64: B64.encode(server_bytes),
        }
    })
}

fn fresh_app() -> Router {
    app(AppState::new(), "test")
}

// Verschlüsselt einen Score (u16) und gibt ihn als Base64(bincode(FheUint16)) zurück
fn enc_score(value: u16) -> String {
    let ck = &keys().client_key;
    let enc = FheUint16::try_encrypt(value, ck).expect("encrypt score");
    B64.encode(bincode::serialize(&enc).expect("serialize"))
}

// Verschlüsselt eine Spieler-ID (u8)
fn enc_id(value: u8) -> String {
    let ck = &keys().client_key;
    let enc = FheUint8::try_encrypt(value, ck).expect("encrypt id");
    B64.encode(bincode::serialize(&enc).expect("serialize"))
}

// Entschlüsselt einen verschlüsselten u16-Score aus Base64
fn dec_score(b64: &str) -> u16 {
    let bytes = B64.decode(b64).expect("decode");
    let enc: FheUint16 = bincode::deserialize(&bytes).expect("deser");
    enc.decrypt(&keys().client_key)
}

// Entschlüsselt eine verschlüsselte u8-ID aus Base64
fn dec_id(b64: &str) -> u8 {
    let bytes = B64.decode(b64).expect("decode");
    let enc: FheUint8 = bincode::deserialize(&bytes).expect("deser");
    enc.decrypt(&keys().client_key)
}

// Entschlüsselt einen verschlüsselten Bool aus Base64
fn dec_bool(b64: &str) -> bool {
    let bytes = B64.decode(b64).expect("decode");
    let enc: FheBool = bincode::deserialize(&bytes).expect("deser");
    enc.decrypt(&keys().client_key)
}

// HTTP-Helfer: GET → (Status, JSON)
async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

// HTTP-Helfer: GET → (Status, body als String) — für Endpoints ohne JSON-Antwort
async fn get_text(app: &Router, uri: &str) -> (StatusCode, String) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

// HTTP-Helfer: POST mit JSON-Body
async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

// Fertig konfigurierten Raum mit gültigen Keys anlegen — gibt den Raumcode zurück
async fn create_room(app: &Router) -> String {
    let body = json!({
        "server_key": &keys().server_key_b64,
        // Server speichert public_key roh — Tests brauchen keinen echten Public-Key
        "public_key": "AAAA",
    });
    let (status, value) = post_json(app, "/create", body).await;
    assert_eq!(status, StatusCode::OK, "create failed: {value}");
    value["code"].as_str().unwrap().to_string()
}

// Wartet bis der Hintergrund-Sort eine erwartete Top-Score in /entries liefert
// (Polling) — gibt die finalen Entries zurück. Schlägt nach `timeout` fehl.
async fn wait_for_top_score(
    app: &Router,
    code: &str,
    expected_top: u16,
    timeout: Duration,
) -> Vec<Value> {
    let deadline = Instant::now() + timeout;
    loop {
        let (status, value) = get_json(app, &format!("/{code}/entries")).await;
        assert_eq!(status, StatusCode::OK);
        let entries = value["entries"].as_array().cloned().unwrap_or_default();
        if let Some(first) = entries.first() {
            let s = dec_score(first["encrypted_score"].as_str().unwrap());
            if s == expected_top {
                return entries;
            }
        }
        if Instant::now() >= deadline {
            panic!("sort did not produce top score {expected_top} within {timeout:?}");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Health-Endpoints aus dem `health`-Crate sind eingebunden und antworten korrekt
#[tokio::test]
async fn health_endpoints_respond() {
    let app = fresh_app();
    assert_eq!(
        get_text(&app, "/healthz").await,
        (StatusCode::OK, "ok".into())
    );
    assert_eq!(
        get_text(&app, "/readyz").await,
        (StatusCode::OK, "ok".into())
    );
    assert_eq!(
        get_text(&app, "/version").await,
        (StatusCode::OK, "test".into())
    );
}

// /create validiert die Eingabe: Bad-Base64 in beiden Feldern + valider B64 mit
// kaputten Bytes (ServerKey-Decompress muss fehlschlagen).
#[tokio::test]
async fn create_rejects_invalid_input() {
    let app = fresh_app();

    // Bad Base64 im server_key
    let (status, _) = post_json(
        &app,
        "/create",
        json!({"server_key": "not*b64!", "public_key": "AAAA"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Bad Base64 im public_key
    let (status, _) = post_json(
        &app,
        "/create",
        json!({"server_key": "AAAA", "public_key": "@@@@"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Valides Base64, aber Bytes sind kein CompressedServerKey
    let garbage = B64.encode([0u8; 32]);
    let (status, _) = post_json(
        &app,
        "/create",
        json!({"server_key": garbage, "public_key": "AAAA"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// Alle Endpoints geben 404 für unbekannten Raumcode zurück.
#[tokio::test]
async fn endpoints_return_404_for_unknown_room() {
    let app = fresh_app();
    let unknown = "999999";

    assert_eq!(
        get_text(&app, &format!("/{unknown}/public-key")).await.0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        get_text(&app, &format!("/{unknown}/entries")).await.0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        post_json(
            &app,
            &format!("/{unknown}/submit"),
            json!({"player_key":"x","encrypted_score":"AAAA","encrypted_id":"AAAA"}),
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
    );
    assert_eq!(
        post_json(
            &app,
            &format!("/{unknown}/rank"),
            json!({"encrypted_id":"AAAA"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND,
    );
}

// /entries und /rank auf einem leeren Leaderboard antworten erfolgreich
// mit leeren Listen — keine FHE-Op wird ausgelöst, kein Fehler.
#[tokio::test]
async fn empty_leaderboard_returns_empty_entries_and_matches() {
    let app = fresh_app();
    let code = create_room(&app).await;

    // /entries trifft den Fallback-Zweig (sorted leer → Insertion-Order = leer)
    let (status, value) = get_json(&app, &format!("/{code}/entries")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["entries"].as_array().unwrap().len(), 0);

    // /rank ohne Einträge → leere Treffer-Liste (kurzschließt vor FHE-Aufruf)
    let (status, value) = post_json(
        &app,
        &format!("/{code}/rank"),
        json!({"encrypted_id": enc_id(7)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["matches"].as_array().unwrap().len(), 0);
}

// Voller Lifecycle-Test (das Herzstück):
//   1. Raum erstellen, Public-Key wieder abholen
//   2. Spieler A reichst 50 ein, dann nochmal 30 → FHE-max behält 50
//   3. Spieler B reichst 99 ein
//   4. Hintergrund-Sort wartet ab, Liste muss absteigend sein: 99, 50
//   5. Rang-Abfrage für ID 9 → Treffer auf Position 1
//   6. Rang-Abfrage für unbekannte ID → keine Treffer
//   7. Re-Submit von A mit kaputtem Ciphertext → 500 (FHE-Deser fehlgeschlagen)
//   8. Rank mit kaputtem Ciphertext → 500
#[tokio::test]
async fn full_lifecycle_create_submit_max_sort_rank() {
    // Runtime-Guard (statt #[cfg(debug_assertions)] panic!), damit der Compiler
    // den Rest der Funktion nicht als unreachable markiert — die IDE würde sonst
    // dauerhaft Unreachable-Warnings im ganzen Test anzeigen.
    if cfg!(debug_assertions) {
        panic!(
            "FHE integration test requires --release: \
             rerun with `cargo test --release` (debug FHE is ~10× slower)"
        );
    }

    let app = fresh_app();
    let code = create_room(&app).await;

    // Public-Key kommt unverändert zurück
    let (status, value) = get_json(&app, &format!("/{code}/public-key")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["public_key"].as_str().unwrap(), "AAAA");

    // Spieler A: 50, dann 30 — max muss 50 behalten
    let a_score_high = enc_score(50);
    let a_score_low = enc_score(30);
    let a_id = enc_id(5);
    let (s, _) = post_json(
        &app,
        &format!("/{code}/submit"),
        json!({"player_key":"A","encrypted_score":a_score_high,"encrypted_id":a_id}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = post_json(
        &app,
        &format!("/{code}/submit"),
        json!({"player_key":"A","encrypted_score":a_score_low,"encrypted_id":a_id}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // Spieler B: 99
    let (s, _) = post_json(
        &app,
        &format!("/{code}/submit"),
        json!({"player_key":"B","encrypted_score":enc_score(99),"encrypted_id":enc_id(9)}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);

    // Auf den Sort warten — n=2 → ~1 Layer × ein paar Sekunden
    let entries = wait_for_top_score(&app, &code, 99, Duration::from_secs(120)).await;
    assert_eq!(entries.len(), 2, "{entries:?}");
    assert_eq!(
        dec_score(entries[0]["encrypted_score"].as_str().unwrap()),
        99
    );
    assert_eq!(dec_id(entries[0]["encrypted_id"].as_str().unwrap()), 9);
    assert_eq!(
        dec_score(entries[1]["encrypted_score"].as_str().unwrap()),
        50
    );
    assert_eq!(dec_id(entries[1]["encrypted_id"].as_str().unwrap()), 5);

    // Rang-Abfrage für ID 9 — Treffer auf Position 1, miss auf Position 2
    let (status, value) = post_json(
        &app,
        &format!("/{code}/rank"),
        json!({"encrypted_id": enc_id(9)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let matches: Vec<bool> = value["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| dec_bool(v.as_str().unwrap()))
        .collect();
    assert_eq!(matches, vec![true, false]);

    // Rang-Abfrage für unbekannte ID — alles false
    let (status, value) = post_json(
        &app,
        &format!("/{code}/rank"),
        json!({"encrypted_id": enc_id(123)}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let matches: Vec<bool> = value["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| dec_bool(v.as_str().unwrap()))
        .collect();
    assert_eq!(matches, vec![false, false]);

    // Re-Submit für Spieler A mit kaputtem Score → FHE-Deser-Fehler in keep_max
    let garbage = B64.encode([0u8; 16]);
    let (s, _) = post_json(
        &app,
        &format!("/{code}/submit"),
        json!({"player_key":"A","encrypted_score":garbage.clone(),"encrypted_id":a_id}),
    )
    .await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);

    // Rank mit kaputter ID → FHE-Deser-Fehler in rank_matches
    let (s, _) = post_json(
        &app,
        &format!("/{code}/rank"),
        json!({"encrypted_id": garbage}),
    )
    .await;
    assert_eq!(s, StatusCode::INTERNAL_SERVER_ERROR);
}

// Volles Leaderboard: 20 Plätze passen, der 21. Spieler bekommt 409.
// Wir senden `MAX_ENTRIES`-fach denselben Ciphertext — keine teure FHE-Op
// wird ausgelöst (nur neue Spieler, deshalb kein keep_max).
#[tokio::test]
async fn leaderboard_rejects_player_when_full() {
    let app = fresh_app();
    let code = create_room(&app).await;

    let same_score = enc_score(1);
    let same_id = enc_id(1);

    for i in 0..20u32 {
        let (s, _) = post_json(
            &app,
            &format!("/{code}/submit"),
            json!({
                "player_key": format!("p{i}"),
                "encrypted_score": same_score,
                "encrypted_id": same_id,
            }),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "submit {i} failed");
    }

    // 21. Spieler — Raum voll
    let (s, _) = post_json(
        &app,
        &format!("/{code}/submit"),
        json!({
            "player_key": "overflow",
            "encrypted_score": same_score,
            "encrypted_id": same_id,
        }),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

// Der Janitor entfernt Sessions, an denen länger als `idle_timeout` keine
// Aktivität war. Test:
//   1. Raum anlegen.
//   2. `last_access` der Session manuell auf 0 (Unix-Epoch) zurücksetzen —
//      so wirkt sie für den Janitor sofort uralt.
//   3. Janitor mit kurzem Intervall (50 ms) starten, kurz warten.
//   4. Verifizieren, dass /public-key jetzt 404 liefert (Session ist weg).
//
// Wir bauen `state` und `app` hier separat (statt `fresh_app`), damit wir
// den geteilten `AppState` zum Reingreifen behalten — `AppState` ist `Clone`,
// beide Klone teilen sich dieselbe `Arc<RwLock<HashMap>>`.
#[tokio::test]
async fn janitor_evicts_idle_sessions() {
    let state = AppState::new();
    let app_router = app(state.clone(), "test");

    // Echten Raum anlegen — braucht einen gültigen ServerKey (aus dem geteilten
    // TestKeys-Fixture, das beim ersten Aufruf einmalig erzeugt wird).
    let body = json!({
        "server_key": &keys().server_key_b64,
        "public_key": "AAAA",
    });
    let (status, value) = post_json(&app_router, "/create", body).await;
    assert_eq!(status, StatusCode::OK, "create failed: {value}");
    let code = value["code"].as_str().unwrap().to_string();

    // Sanity-Check: Raum existiert direkt nach /create
    let (status, _) = get_json(&app_router, &format!("/{code}/public-key")).await;
    assert_eq!(status, StatusCode::OK);

    // `state.get()` touched die Session auf jetzt — wir überschreiben sie direkt
    // danach auf 0, damit der nächste Janitor-Scan sie als idle einstuft.
    // Der Arc-Clone darf danach gedroppt werden; der Eintrag in der Map bleibt
    // und zeigt auf dieselbe Session-Instanz.
    {
        let session = state.get(&code).await.expect("session exists");
        session.last_access.store(0, Ordering::Relaxed);
    }

    // Janitor mit 60 s Idle-Timeout: da `last_access = 0` (Epoch) ist, gilt die
    // Session sofort als ~50+ Jahre idle und wird beim ersten Tick entfernt.
    state.spawn_janitor(Duration::from_secs(60), Duration::from_millis(50));

    // Genug Zeit für mindestens einen Janitor-Tick (`interval`-Tick wird vom
    // Janitor-Code übersprungen, d.h. erster echter Scan kommt nach ~50 ms).
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Session muss weg sein — alle Endpoints liefern jetzt 404.
    let (status, _) = get_text(&app_router, &format!("/{code}/public-key")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "session should have been evicted by janitor"
    );
}

// Submit mit ungültigem Base64 wird vor jeder FHE-Op abgewiesen (400).
#[tokio::test]
async fn submit_rejects_bad_base64() {
    let app = fresh_app();
    let code = create_room(&app).await;
    let (s, _) = post_json(
        &app,
        &format!("/{code}/submit"),
        json!({"player_key":"x","encrypted_score":"!!!","encrypted_id":"AAAA"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
