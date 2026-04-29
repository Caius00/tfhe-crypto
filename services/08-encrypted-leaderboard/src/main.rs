use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tfhe::{prelude::*, CompressedServerKey, FheUint16, FheUint8};
use tokio::sync::RwLock;

// ---------------------------------------------------------------------------
// Zustand
// ---------------------------------------------------------------------------

// Alle Räume: Raumcode → Session, thread-sicher hinter RwLock
type AppState = Arc<RwLock<HashMap<String, Session>>>;

struct Session {
    server_key_bytes: Vec<u8>,       // erlaubt FHE-Ops, aber kein Entschlüsseln
    public_key_bytes: Vec<u8>,       // wird an Spieler weitergegeben zum Verschlüsseln
    entries: Vec<Entry>,             // Quelle der Wahrheit: player_key ↔ Score immer gepaart
    sorted: Vec<(Vec<u8>, Vec<u8>)>, // Anzeigereihenfolge, vom Hintergrund-Sort befüllt
}

#[derive(Clone)]
struct Entry {
    player_key: String, // Klartext-ID, nur für Deduplizierung
    enc_score: Vec<u8>, // verschlüsselter Score (FheUint16)
    enc_id: Vec<u8>,    // verschlüsselte Spieler-ID (FheUint32)
}

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreateRequest {
    server_key: String,
    public_key: String,
}

#[derive(Serialize)]
struct CreateResponse {
    code: String,
}

#[derive(Deserialize)]
struct SubmitRequest {
    player_key: String,
    encrypted_score: String,
    encrypted_id: String,
}

#[derive(Serialize)]
struct PublicKeyResponse {
    public_key: String,
}

#[derive(Serialize)]
struct EntryDto {
    encrypted_score: String,
    encrypted_id: String,
}

#[derive(Serialize)]
struct EntriesResponse {
    entries: Vec<EntryDto>,
}

type ApiError = (StatusCode, String);

// ---------------------------------------------------------------------------
// Hilfsfunktionen
// ---------------------------------------------------------------------------

// Base64 ↔ rohe Bytes
fn b64_decode(s: &str) -> Result<Vec<u8>, ApiError> {
    general_purpose::STANDARD
        .decode(s)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Base64 error: {e}")))
}

fn b64_encode(b: &[u8]) -> String {
    general_purpose::STANDARD.encode(b)
}

// 6-stelliger Raumcode aus aktueller Zeit
fn generate_code() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let n = (t.as_secs() ^ (t.subsec_nanos() as u64)) % 900_000 + 100_000;
    format!("{n}")
}

// Server-Key in den thread-lokalen Speicher von tfhe-rs laden
fn load_server_key(bytes: &[u8]) -> Result<(), String> {
    let compressed: CompressedServerKey = bincode::deserialize(bytes)
        .map_err(|e| format!("Invalid server key: {e}"))?;
    tfhe::set_server_key(compressed.decompress());
    Ok(())
}

// ---------------------------------------------------------------------------
// FHE-Operationen
// ---------------------------------------------------------------------------

// Gibt den höheren der beiden verschlüsselten Scores zurück (+ zugehörige ID)
fn fhe_keep_max(
    old_s: &[u8],
    old_i: &[u8],
    new_s: &[u8],
    new_i: &[u8],
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let old_score: FheUint16 =
        bincode::deserialize(old_s).map_err(|e| format!("Deserialize old_score: {e}"))?;
    let new_score: FheUint16 =
        bincode::deserialize(new_s).map_err(|e| format!("Deserialize new_score: {e}"))?;
    let old_id: FheUint8 =
        bincode::deserialize(old_i).map_err(|e| format!("Deserialize old_id: {e}"))?;
    let new_id: FheUint8 =
        bincode::deserialize(new_i).map_err(|e| format!("Deserialize new_id: {e}"))?;

    let new_is_better = old_score.lt(&new_score);
    let kept_score = new_is_better.if_then_else(&new_score, &old_score);
    let kept_id = new_is_better.if_then_else(&new_id, &old_id);

    Ok((
        bincode::serialize(&kept_score).map_err(|e| format!("Serialize score: {e}"))?,
        bincode::serialize(&kept_id).map_err(|e| format!("Serialize id: {e}"))?,
    ))
}

// Erzeugt alle Layer eines Batcher's Odd-Even Mergesort Networks für n Elemente.
// Jede Layer ist eine Liste unabhängiger (i, j)-Vergleichspaare die parallel laufen können.
fn batcher_layers(n: usize) -> Vec<Vec<(usize, usize)>> {
    let mut layers: Vec<Vec<(usize, usize)>> = Vec::new();

    let mut t = 1;
    while t < n {
        t *= 2;
    }

    let mut p = t / 2;
    while p > 0 {
        let mut q = t / 2;
        let mut r = 0;
        let mut d = p;

        loop {
            let mut layer: Vec<(usize, usize)> = Vec::new();
            let mut i = r;
            while i + d < n {
                if (i & p) == r {
                    layer.push((i, i + d));
                }
                i += 1;
            }
            if !layer.is_empty() {
                layers.push(layer);
            }

            if q == p {
                break;
            }
            d = q - p;
            q /= 2;
            r = p;
        }

        p /= 2;
    }

    layers
}

// Batcher's Odd-Even Mergesort mit Rayon-Parallelismus.
// Jede Layer wird parallel verarbeitet: alle unabhängigen Vergleiche gleichzeitig auf
// separaten CPU-Cores. Reduziert die Zeit von O(n²) auf O(log²n) Runden × Parallelität.
fn fhe_sort(pairs: &mut [(Vec<u8>, Vec<u8>)]) -> Result<(), String> {
    let n = pairs.len();
    if n < 2 {
        return Ok(());
    }

    let layers = batcher_layers(n);

    type SwapEntry = (usize, usize, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);

    for layer in layers {

        // Alle Vergleiche dieser Layer parallel berechnen; Fehler werden gesammelt
        let swapped: Result<Vec<SwapEntry>, String> = layer
                .par_iter()
                .map(|&(i, j)| {
                    let s_i: FheUint16 = bincode::deserialize(&pairs[i].0)
                        .map_err(|e| format!("Deserialize score[{i}]: {e}"))?;
                    let s_j: FheUint16 = bincode::deserialize(&pairs[j].0)
                        .map_err(|e| format!("Deserialize score[{j}]: {e}"))?;
                    let id_i: FheUint8 = bincode::deserialize(&pairs[i].1)
                        .map_err(|e| format!("Deserialize id[{i}]: {e}"))?;
                    let id_j: FheUint8 = bincode::deserialize(&pairs[j].1)
                        .map_err(|e| format!("Deserialize id[{j}]: {e}"))?;

                    let swap = s_i.lt(&s_j);
                    let new_s_i = bincode::serialize(&swap.if_then_else(&s_j, &s_i))
                        .map_err(|e| format!("Serialize score[{i}]: {e}"))?;
                    let new_s_j = bincode::serialize(&swap.if_then_else(&s_i, &s_j))
                        .map_err(|e| format!("Serialize score[{j}]: {e}"))?;
                    let new_id_i = bincode::serialize(&swap.if_then_else(&id_j, &id_i))
                        .map_err(|e| format!("Serialize id[{i}]: {e}"))?;
                    let new_id_j = bincode::serialize(&swap.if_then_else(&id_i, &id_j))
                        .map_err(|e| format!("Serialize id[{j}]: {e}"))?;

                    Ok((i, j, new_s_i, new_s_j, new_id_i, new_id_j))
                })
                .collect();

        // Ergebnisse sequenziell zurückschreiben (keine Überlappung zwischen Paaren garantiert)
        for (i, j, new_s_i, new_s_j, new_id_i, new_id_j) in swapped? {
            pairs[i] = (new_s_i, new_id_i);
            pairs[j] = (new_s_j, new_id_j);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

// Creator lädt Server- und Public-Key hoch → erhält 6-stelligen Raumcode
async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateRequest>,
) -> Result<Json<CreateResponse>, ApiError> {
    let server_key_bytes = b64_decode(&req.server_key)?;
    let public_key_bytes = b64_decode(&req.public_key)?;

    let _: CompressedServerKey = bincode::deserialize(&server_key_bytes)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid server key: {e}")))?;

    let code = generate_code();
    state.write().await.insert(
        code.clone(),
        Session {
            server_key_bytes,
            public_key_bytes,
            entries: vec![],
            sorted: vec![],
        },
    );
    Ok(Json(CreateResponse { code }))
}

// Gibt den Public-Key des Raums zurück, damit Spieler ihre Scores verschlüsseln können
async fn get_public_key(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> Result<Json<PublicKeyResponse>, ApiError> {
    let sessions = state.read().await;
    let s = sessions
        .get(&code)
        .ok_or((StatusCode::NOT_FOUND, "Session not found".into()))?;
    Ok(Json(PublicKeyResponse {
        public_key: b64_encode(&s.public_key_bytes),
    }))
}

// Spieler reicht verschlüsselten Score ein:
//   Neu      → direkt eintragen
//   Bekannt  → FHE max(alt, neu), ~3–5 s synchron
// Danach: Hintergrund-Sort aktualisiert `sorted` (nie `entries`)
async fn submit_score(
    State(state): State<AppState>,
    Path(code): Path<String>,
    Json(req): Json<SubmitRequest>,
) -> Result<StatusCode, ApiError> {
    let new_s = b64_decode(&req.encrypted_score)?;
    let new_i = b64_decode(&req.encrypted_id)?;

    // Aktuellen Score des Spielers lesen
    let (old_entry, server_key_bytes) = {
        let sessions = state.read().await;
        let session = sessions
            .get(&code)
            .ok_or((StatusCode::NOT_FOUND, "Session not found".into()))?;
        let old = session
            .entries
            .iter()
            .find(|e| e.player_key == req.player_key)
            .map(|e| (e.enc_score.clone(), e.enc_id.clone()));
        (old, session.server_key_bytes.clone())
    };

    // FHE-Vergleich für bekannte Spieler; neue Spieler überspringen
    let (kept_s, kept_i) = match old_entry {
        Some((old_s, old_i)) => tokio::task::block_in_place(|| {
            load_server_key(&server_key_bytes)
                .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
            fhe_keep_max(&old_s, &old_i, &new_s, &new_i)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
        })?,
        None => (new_s, new_i),
    };

    // Besten Score in entries schreiben
    {
        let mut sessions = state.write().await;
        let session = sessions
            .get_mut(&code)
            .ok_or((StatusCode::NOT_FOUND, "Session not found".into()))?;

        match session
            .entries
            .iter_mut()
            .find(|e| e.player_key == req.player_key)
        {
            Some(e) => {
                e.enc_score = kept_s;
                e.enc_id = kept_i;
            }
            None => {
                if session.entries.len() >= 20 {
                    return Err((StatusCode::CONFLICT, "Leaderboard is full (max 20)".into()));
                }
                session.entries.push(Entry {
                    player_key: req.player_key,
                    enc_score: kept_s,
                    enc_id: kept_i,
                });
            }
        }
    }

    // Hintergrund-Sort: nur `sorted` wird aktualisiert, `entries` bleibt unberührt
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = {
            let sessions = state_clone.read().await;
            sessions
                .get(&code)
                .map(|s| {
                    s.entries
                        .iter()
                        .map(|e| (e.enc_score.clone(), e.enc_id.clone()))
                        .collect()
                })
                .unwrap_or_default()
        };

        let sort_ok = tokio::task::block_in_place(|| {
            load_server_key(&server_key_bytes)
                .map_err(|e| eprintln!("Sort: failed to load server key: {e}"))
                .and_then(|_| {
                    fhe_sort(&mut pairs).map_err(|e| eprintln!("Sort failed: {e}"))
                })
                .is_ok()
        });

        if sort_ok {
            let mut sessions = state_clone.write().await;
            if let Some(session) = sessions.get_mut(&code) {
                session.sorted = pairs;
            }
        }
    });

    Ok(StatusCode::OK)
}

// Gibt die Einträge zurück: sortiert wenn Sort fertig, sonst Einfügereihenfolge
async fn get_entries(
    State(state): State<AppState>,
    Path(code): Path<String>,
) -> Result<Json<EntriesResponse>, ApiError> {
    let sessions = state.read().await;
    let session = sessions
        .get(&code)
        .ok_or((StatusCode::NOT_FOUND, "Session not found".into()))?;

    let pairs: Vec<(&[u8], &[u8])> = if !session.sorted.is_empty() {
        session
            .sorted
            .iter()
            .map(|(s, i)| (s.as_slice(), i.as_slice()))
            .collect()
    } else {
        session
            .entries
            .iter()
            .map(|e| (e.enc_score.as_slice(), e.enc_id.as_slice()))
            .collect()
    };

    let entries = pairs
        .into_iter()
        .map(|(s, i)| EntryDto {
            encrypted_score: b64_encode(s),
            encrypted_id: b64_encode(i),
        })
        .collect();

    Ok(Json(EntriesResponse { entries }))
}

// ---------------------------------------------------------------------------
// Einstiegspunkt
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    let state: AppState = Arc::new(RwLock::new(HashMap::new()));

    let app = Router::new()
        .route("/create", post(create_session))
        .route("/{code}/public-key", get(get_public_key))
        .route("/{code}/submit", post(submit_score))
        .route("/{code}/entries", get(get_entries))
        .with_state(state)
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)); // FHE-Schlüssel können mehrere MB groß sein

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Leaderboard service on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
