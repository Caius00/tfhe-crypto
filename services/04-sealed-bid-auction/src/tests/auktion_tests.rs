#[cfg(test)]
use crate::auction::{self, types::*, BIDS};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
    Router,
};
use base64::{engine::general_purpose, Engine as _};
use serde_json::{json, Value};
use serial_test::serial;
use std::sync::OnceLock;
use tfhe::{
    generate_keys, prelude::*, ClientKey, CompressedServerKey, ConfigBuilder, FheBool, FheUint32,
};
use tower::util::ServiceExt;

// ── Gecachter ServerKey für die Testumgebung ──────────────────────────────
static TFHE_SETUP: OnceLock<(ClientKey, String)> = OnceLock::new();
fn get_tfhe_setup() -> &'static (ClientKey, String) {
    TFHE_SETUP.get_or_init(|| {
        let config = ConfigBuilder::default().build();
        let (client_key, _) = generate_keys(config);
        let compressed = CompressedServerKey::new(&client_key);
        let sk_bytes = bincode::serialize(&compressed).unwrap();
        let sk_b64 = general_purpose::STANDARD.encode(&sk_bytes);
        (client_key, sk_b64)
    })
}

/// Hilfsfunktion, bereinigung vor jedem test
fn clear_bids() {
    let mut liste = BIDS.lock().unwrap();
    liste.clear();
}

fn build_app() -> Router {
    Router::new()
        .route("/gebot", axum::routing::post(auction::gebot_empfangen))
        .route("/auswerten", axum::routing::get(auction::auktion_auswerten))
        // KORREKTUR: Hebt das 2MB-Limit für den HTTP-Body in der Testumgebung auf!
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
}

// ── Hilfsfunktionen: HTTP-Mocking-Requests ───────────────────────────────
async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()));
    (status, json)
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let req = Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()));
    (status, json)
}

// TEST 1: Erfolgreicher Auktions-Durchlauf (Verschlüsseln -> Senden -> Auswerten)

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn test_auction_successful_flow() {
    clear_bids();
    let app = build_app();
    let (client_key, sk_b64) = get_tfhe_setup();

    // Gebote verschlüsseln (Bieter A bietet 500, Bieter B bietet 750)
    let enc_amount_a = FheUint32::encrypt(500u32, client_key);
    let enc_amount_b = FheUint32::encrypt(750u32, client_key);

    let payload_a = json!({
        "bidder_name": "Bieter_A",
        "encrypted_amount": general_purpose::STANDARD.encode(bincode::serialize(&enc_amount_a).unwrap()),
        "server_key": sk_b64
    });

    let payload_b = json!({
        "bidder_name": "Bieter_B",
        "encrypted_amount": general_purpose::STANDARD.encode(bincode::serialize(&enc_amount_b).unwrap()),
        "server_key": sk_b64
    });

    // Gebot A senden
    let (status_a, body_a) = post_json(&app, "/gebot", payload_a).await;
    assert_eq!(status_a, StatusCode::OK);
    assert!(body_a["response"].as_str().unwrap().contains("erfolgreich"));

    //  Gebot B senden
    let (status_b, body_b) = post_json(&app, "/gebot", payload_b).await;
    assert_eq!(status_b, StatusCode::OK);

    // Blinde Auswertung starten
    let (status_eval, body_eval) = get_json(&app, "/auswerten").await;
    assert_eq!(status_eval, StatusCode::OK);
    assert!(body_eval["status"].as_str().unwrap().contains("2 Geboten"));

    let encrypted_result_b64 = body_eval["encrypted_result"].as_str().unwrap();

    let result_bytes = general_purpose::STANDARD
        .decode(encrypted_result_b64)
        .unwrap();

    let ist_b_groesser: FheBool = bincode::deserialize(&result_bytes).unwrap();

    let ergebnis: bool = ist_b_groesser.decrypt(client_key);

    assert!(
            ergebnis, 
            "Mathematischer FHE-Check fehlgeschlagen: Gebot B (750) muss als größer als Gebot A (500) evaluiert werden!"
        );

    println!("✅ test: Vollständiger FHE-Auktionsdurchlauf inklusive Entschlüsselung erfolgreich!");
}

// TEST 2: Fehlerfall - Auswertung ohne Gebote

#[tokio::test]
async fn test_error_evaluation_empty_list() {
    clear_bids();
    let app = build_app();

    let (status, _) = get_json(&app, "/auswerten").await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    println!("✅ test: Leere Liste wird bei Auswertung korrekt blockiert!");
}

// TEST 3: Fehlerfall - Defektes Base64 Format

#[tokio::test]
async fn test_error_invalid_base64_format() {
    clear_bids();
    let app = build_app();
    let (_, sk_b64) = get_tfhe_setup();

    let malformed_payload = json!({
        "bidder_name": "Hacker",
        "encrypted_amount": "!!!kein-base64-format!!!",
        "server_key": sk_b64
    });

    let (status, _) = post_json(&app, "/gebot", malformed_payload).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    println!("✅test: Falsches Base64-Format wird sicher abgefangen!");
}
