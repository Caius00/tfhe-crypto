use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::codec::{b64_decode, b64_encode};
use crate::fhe::FheEngine;
use crate::state::{AppState, EncEntry, Entry, Session, MAX_ENTRIES};

pub type ApiError = (StatusCode, String);

// ---------------------------------------------------------------------------
// Request-/Response-Typen
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub struct CreateRequest {
    /// Base64-kodierter `CompressedServerKey` (bincode-serialisiert).
    pub server_key: String,
    /// Base64-kodierter Public-Key (an Spieler weitergereicht, sonst unverändert).
    pub public_key: String,
}

#[derive(Serialize, JsonSchema)]
pub struct CreateResponse {
    /// 6-stelliger Raumcode, den E mit Spielern teilt.
    pub code: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CodePath {
    /// 6-stelliger Raumcode des Leaderboards.
    pub code: String,
}

#[derive(Serialize, JsonSchema)]
pub struct PublicKeyResponse {
    pub public_key: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SubmitRequest {
    /// Klartext-Schlüssel des Spielers (für Server-seitiges Dedup).
    pub player_key: String,
    /// Base64-kodierter `FheUint16`.
    pub encrypted_score: String,
    /// Base64-kodierter `FheUint8`.
    pub encrypted_id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct EntryDto {
    pub encrypted_score: String,
    pub encrypted_id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct EntriesResponse {
    pub entries: Vec<EntryDto>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RankRequest {
    pub encrypted_id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct RankResponse {
    /// Pro Position der sortierten Liste: ein verschlüsselter Bool
    /// (true = die Kennung passt). 1-basierter Rang = Index + 1.
    pub matches: Vec<String>,
}

// ---------------------------------------------------------------------------
// Fehler-Helfer
// ---------------------------------------------------------------------------

fn bad_req<E: std::fmt::Display>(e: E) -> ApiError {
    (StatusCode::BAD_REQUEST, e.to_string())
}
fn server_err<E: std::fmt::Display>(e: E) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}
fn not_found() -> ApiError {
    (StatusCode::NOT_FOUND, "Session not found".into())
}
async fn require_session(state: &AppState, code: &str) -> Result<Arc<Session>, ApiError> {
    state.get(code).await.ok_or_else(not_found)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

// Erstellt einen Raum: dekomprimiert den ServerKey einmalig und legt eine Session an.
// Antwortet mit einem 6-stelligen Raumcode, den E mit Spielern teilen kann.
#[tracing::instrument(skip_all)]
pub async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateRequest>,
) -> Result<Json<CreateResponse>, ApiError> {
    let server_key_bytes = b64_decode(&req.server_key).map_err(bad_req)?;
    // public_key wird unverändert weitergereicht — hier nur Format-Check
    b64_decode(&req.public_key).map_err(bad_req)?;

    // ServerKey-Decompress ist teuer (~hunderte MB, mehrere Sekunden) → blocking pool
    let engine =
        tokio::task::spawn_blocking(move || FheEngine::from_compressed_bytes(&server_key_bytes))
            .await
            .map_err(server_err)?
            .map_err(bad_req)?;

    let session = Arc::new(Session {
        engine: Arc::new(engine),
        public_key_b64: req.public_key,
        entries: Default::default(),
        sorted: Default::default(),
        sort_state: Default::default(),
    });

    let code = state.insert_with_unique_code(session).await;
    Ok(Json(CreateResponse { code }))
}

// Liefert den Public-Key des Raums an Spieler zurück.
pub async fn get_public_key(
    State(state): State<AppState>,
    Path(CodePath { code }): Path<CodePath>,
) -> Result<Json<PublicKeyResponse>, ApiError> {
    let session = require_session(&state, &code).await?;
    Ok(Json(PublicKeyResponse {
        public_key: session.public_key_b64.clone(),
    }))
}

// Spieler reicht einen verschlüsselten Score ein.
//   - Neuer Spieler: direkt aufnehmen (sofern der Raum nicht voll ist).
//   - Bekannter Spieler: FHE-Maximum von alt und neu wird übernommen.
// Ein Hintergrund-Sort wird angetriggert (Single-Flight, siehe spawn_sort_if_idle).
#[tracing::instrument(skip_all, fields(code = %code))]
pub async fn submit_score(
    State(state): State<AppState>,
    Path(CodePath { code }): Path<CodePath>,
    Json(req): Json<SubmitRequest>,
) -> Result<StatusCode, ApiError> {
    let new_score = b64_decode(&req.encrypted_score).map_err(bad_req)?;
    let new_id = b64_decode(&req.encrypted_id).map_err(bad_req)?;

    let session = require_session(&state, &code).await?;

    // Eventuell vorhandenen Eintrag holen, ohne Schreib-Lock über die FHE-Op zu halten
    let existing: Option<EncEntry> = session
        .entries
        .read()
        .await
        .iter()
        .find(|e| e.player_key == req.player_key)
        .map(|e| e.enc.clone());

    // Bei Re-Submit: blockierender FHE-Vergleich auf dem dedizierten Pool
    let kept = match existing {
        Some(old) => {
            let engine = Arc::clone(&session.engine);
            tokio::task::spawn_blocking(move || {
                engine.keep_max(&old.score, &old.id, &new_score, &new_id)
            })
            .await
            .map_err(server_err)?
            .map_err(server_err)?
        }
        None => (new_score, new_id),
    };

    // Eintrag persistieren (kurzer Schreib-Lock)
    {
        let mut entries = session.entries.write().await;
        match entries.iter_mut().find(|e| e.player_key == req.player_key) {
            Some(e) => {
                e.enc.score = kept.0;
                e.enc.id = kept.1;
            }
            None => {
                if entries.len() >= MAX_ENTRIES {
                    return Err((
                        StatusCode::CONFLICT,
                        format!("Leaderboard is full (max {MAX_ENTRIES})"),
                    ));
                }
                entries.push(Entry {
                    player_key: req.player_key,
                    enc: EncEntry {
                        score: kept.0,
                        id: kept.1,
                    },
                });
            }
        }
    }

    spawn_sort_if_idle(Arc::clone(&session));
    Ok(StatusCode::OK)
}

// Liefert die aktuelle Anzeigereihenfolge: bevorzugt die zuletzt fertig sortierte
// Sicht; falls noch keine vorhanden, die Insertion-Order. Nur E mit dem ClientKey
// kann die Inhalte überhaupt entschlüsseln.
pub async fn get_entries(
    State(state): State<AppState>,
    Path(CodePath { code }): Path<CodePath>,
) -> Result<Json<EntriesResponse>, ApiError> {
    let session = require_session(&state, &code).await?;

    let sorted = session.sorted.read().await;
    let entries: Vec<EntryDto> = if !sorted.is_empty() {
        sorted
            .iter()
            .map(|e| EntryDto {
                encrypted_score: b64_encode(&e.score),
                encrypted_id: b64_encode(&e.id),
            })
            .collect()
    } else {
        drop(sorted); // unnötigen Read-Lock früh freigeben
        session
            .entries
            .read()
            .await
            .iter()
            .map(|e| EntryDto {
                encrypted_score: b64_encode(&e.enc.score),
                encrypted_id: b64_encode(&e.enc.id),
            })
            .collect()
    };

    Ok(Json(EntriesResponse { entries }))
}

// Rang-Abfrage: E sendet eine verschlüsselte Kennung und bekommt für jede
// Position der sortierten Liste einen verschlüsselten Bool zurück (Treffer ja/nein).
// Lokale Auswertung bei E ergibt 0..n Ränge — funktioniert auch bei Mehrfach-Treffern.
#[tracing::instrument(skip_all, fields(code = %code))]
pub async fn query_rank(
    State(state): State<AppState>,
    Path(CodePath { code }): Path<CodePath>,
    Json(req): Json<RankRequest>,
) -> Result<Json<RankResponse>, ApiError> {
    let target = b64_decode(&req.encrypted_id).map_err(bad_req)?;
    let session = require_session(&state, &code).await?;

    // Snapshot ziehen (sortiert wenn vorhanden, sonst Insertion-Order)
    let snapshot: Vec<(Vec<u8>, Vec<u8>)> = {
        let sorted = session.sorted.read().await;
        if !sorted.is_empty() {
            sorted
                .iter()
                .map(|e| (e.score.clone(), e.id.clone()))
                .collect()
        } else {
            drop(sorted);
            session
                .entries
                .read()
                .await
                .iter()
                .map(|e| (e.enc.score.clone(), e.enc.id.clone()))
                .collect()
        }
    };

    if snapshot.is_empty() {
        return Ok(Json(RankResponse { matches: vec![] }));
    }

    let engine = Arc::clone(&session.engine);
    let bool_bytes = tokio::task::spawn_blocking(move || engine.rank_matches(&snapshot, &target))
        .await
        .map_err(server_err)?
        .map_err(server_err)?;

    Ok(Json(RankResponse {
        matches: bool_bytes.iter().map(|b| b64_encode(b)).collect(),
    }))
}

// ---------------------------------------------------------------------------
// Hintergrund-Sort (Single-Flight)
// ---------------------------------------------------------------------------

// Triggert einen Hintergrund-Sort, falls noch keiner läuft. Andernfalls wird
// nur das `dirty`-Flag gesetzt — der laufende Task zieht dann genau einen
// weiteren Pass nach. So fallen Burst-Submits in höchstens "current+1" Sorts
// zusammen, statt sich zu stauen.
fn spawn_sort_if_idle(session: Arc<Session>) {
    use tracing::Instrument;
    let span = tracing::info_span!("background_sort");
    tokio::spawn(async move {
        // Beanspruche den Sort-Slot — oder markiere nur Bedarf, wenn schon gesortet wird
        {
            let mut s = session.sort_state.lock().await;
            if s.running {
                s.dirty = true;
                return;
            }
            s.running = true;
        }

        // Schleife läuft so lange, bis kein neuer Submit während des Sorts kam
        loop {
            let snapshot: Vec<(Vec<u8>, Vec<u8>)> = session
                .entries
                .read()
                .await
                .iter()
                .map(|e| (e.enc.score.clone(), e.enc.id.clone()))
                .collect();

            // Trivial-Fälle ohne FHE-Aufwand
            if snapshot.len() <= 1 {
                let result: Vec<EncEntry> = snapshot
                    .into_iter()
                    .map(|(s, i)| EncEntry { score: s, id: i })
                    .collect();
                *session.sorted.write().await = result;
            } else {
                let engine = Arc::clone(&session.engine);
                let join = tokio::task::spawn_blocking(move || {
                    let mut pairs = snapshot;
                    engine.sort_by_score_desc(&mut pairs).map(|()| pairs)
                })
                .await;

                match join {
                    Ok(Ok(pairs)) => {
                        let result: Vec<EncEntry> = pairs
                            .into_iter()
                            .map(|(s, i)| EncEntry { score: s, id: i })
                            .collect();
                        *session.sorted.write().await = result;
                    }
                    Ok(Err(e)) => eprintln!("[sort] {e}"),
                    Err(e) => eprintln!("[sort] join error: {e}"),
                }
            }

            // Slot freigeben oder nochmal sortieren?
            let mut s = session.sort_state.lock().await;
            if !s.dirty {
                s.running = false;
                return;
            }
            s.dirty = false;
        }
    }.instrument(span));
}
