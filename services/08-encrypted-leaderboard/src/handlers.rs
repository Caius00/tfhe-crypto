//! Axum-Handler des Leaderboard-Services.
//!
//! Fünf Endpunkte:
//! - `POST /create`              — Raum anlegen (ServerKey hochladen, Code bekommen)
//! - `GET  /{code}/public-key`   — Public-Key holen (für Spieler zum Verschlüsseln)
//! - `POST /{code}/submit`       — Verschlüsselten Score einreichen (mit Dedup + FHE-Max)
//! - `GET  /{code}/entries`      — Aktuelle (verschlüsselte) Reihenfolge abrufen
//! - `POST /{code}/rank`         — Rang einer verschlüsselten ID abfragen
//!
//! Wichtige Konventionen für alle Handler:
//! - Lange FHE-Operationen laufen in `tokio::task::spawn_blocking`, damit der
//!   Tokio-Runtime-Thread nicht für Sekunden blockiert wird.
//! - Locks (`entries`, `sorted`) werden NIE über `spawn_blocking` gehalten —
//!   wir clonen die Daten raus, droppen den Lock und arbeiten dann blockierend.
//! - `require_session` erledigt nebenbei das `touch()` der Session, damit der
//!   Janitor sie nicht wegräumt während sie aktiv genutzt wird.

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

/// Standardisierter Fehlertyp aller Handler. axum macht aus dem Tupel
/// automatisch `(StatusCode, body: String)`.
pub type ApiError = (StatusCode, String);

// ---------------------------------------------------------------------------
// Request-/Response-Typen
//
// Alle DTOs leiten zusätzlich zu Serde auch `JsonSchema` ab — `aide` braucht
// das für die automatische OpenAPI-Generierung (`/openapi.json` + Swagger UI).
// ---------------------------------------------------------------------------

#[derive(Deserialize, JsonSchema)]
pub struct CreateRequest {
    /// Base64-kodierter `CompressedServerKey` (bincode-serialisiert).
    /// Wird einmalig beim Raum-Anlegen dekomprimiert — danach lebt der
    /// dekomprimierte Key in der `FheEngine` der Session.
    pub server_key: String,
    /// Base64-kodierter Public-Key. Der Service nutzt ihn nicht selbst,
    /// reicht ihn nur an Spieler weiter, damit die Scores damit verschlüsseln.
    pub public_key: String,
}

#[derive(Serialize, JsonSchema)]
pub struct CreateResponse {
    /// 6-stelliger Raumcode, den E mit Spielern teilt.
    pub code: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CodePath {
    /// 6-stelliger Raumcode des Leaderboards (aus dem URL-Pfad).
    pub code: String,
}

#[derive(Serialize, JsonSchema)]
pub struct PublicKeyResponse {
    pub public_key: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SubmitRequest {
    /// KLARTEXT-Schlüssel des Spielers (für server-seitiges Dedup).
    /// Identifiziert den Spieler innerhalb seines Raums — z.B. "alice", "player_42".
    pub player_key: String,
    /// Base64-kodierter `FheUint16` mit dem aktuellen Score (0..=65535).
    pub encrypted_score: String,
    /// Base64-kodierter `FheUint8` mit der Spieler-ID (0..=255).
    pub encrypted_id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct EntryDto {
    pub encrypted_score: String,
    pub encrypted_id: String,
}

/// Klartext-Roster der bekannten Spieler im Raum (Insertion-Order).
///
/// Wird parallel zur sortierten `entries`-Liste ausgeliefert, damit der Creator
/// nach Entschlüsselung der `encrypted_id` einer sortierten Position den
/// passenden Namen daneben legen kann.
///
/// `player_key` ist bewusst Klartext — er ist es ohnehin schon im Submit-Request
/// (siehe Threat-Model: server-seitiges Dedup-Token).
#[derive(Serialize, JsonSchema)]
pub struct RosterEntryDto {
    pub player_key: String,
    pub encrypted_id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct EntriesResponse {
    pub entries: Vec<EntryDto>,
    pub roster: Vec<RosterEntryDto>,
}

#[derive(Deserialize, JsonSchema)]
pub struct RankRequest {
    pub encrypted_id: String,
}

#[derive(Serialize, JsonSchema)]
pub struct RankResponse {
    /// Pro Position der sortierten Liste: ein verschlüsselter Bool
    /// (true = die Kennung passt). 1-basierter Rang = Index + 1.
    /// Mehrere `true`-Treffer sind möglich, falls mehrere Einträge die
    /// gesuchte ID tragen — der Client wertet das lokal aus.
    pub matches: Vec<String>,
}

// ---------------------------------------------------------------------------
// Fehler-Helfer
//
// Kurze Konvertierer, damit Handler-Code lesbar bleibt:
//   `.map_err(bad_req)?`   → 400 Bad Request
//   `.map_err(server_err)?`→ 500 Internal Server Error
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

/// Schlägt eine Session per Raumcode nach. Aktualisiert nebenbei `last_access`
/// (siehe `AppState::get`), damit aktive Räume nicht weg-evicted werden.
async fn require_session(state: &AppState, code: &str) -> Result<Arc<Session>, ApiError> {
    state.get(code).await.ok_or_else(not_found)
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `POST /create` — Neuen Leaderboard-Raum anlegen.
///
/// Ablauf:
/// 1. ServerKey und Public-Key aus dem Request base64-dekodieren.
/// 2. ServerKey auf dem Blocking-Pool dekomprimieren (~hunderte MB, mehrere Sek).
/// 3. Frische `Session` anlegen und unter einem freien 6-stelligen Code ablegen.
///
/// Der Public-Key wird unverändert gespeichert — er ist nur für Spieler relevant,
/// die ihre Scores damit verschlüsseln.
#[tracing::instrument(skip_all)]
pub async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateRequest>,
) -> Result<Json<CreateResponse>, ApiError> {
    let server_key_bytes = b64_decode(&req.server_key).map_err(bad_req)?;
    // Public-Key wird nur weitergereicht — hier nur Format-Check, kein Parsing.
    b64_decode(&req.public_key).map_err(bad_req)?;

    // Decompress läuft mehrere Sekunden CPU-bound → `spawn_blocking`, damit
    // der Tokio-Runtime-Thread frei bleibt für andere Requests.
    let engine: FheEngine =
        tokio::task::spawn_blocking(move || FheEngine::from_compressed_bytes(&server_key_bytes))
            .await
            .map_err(server_err)? // JoinError (panic im Blocking-Task)
            .map_err(bad_req)?;   // Deserialisierungsfehler (kaputter ServerKey)

    let session = Arc::new(Session::new(Arc::new(engine), req.public_key));
    let code = state.insert_with_unique_code(session).await;
    Ok(Json(CreateResponse { code }))
}

/// `GET /{code}/public-key` — Public-Key des Raums zurückgeben.
///
/// Spieler holen diesen Key, um damit ihre Scores zu verschlüsseln,
/// bevor sie `/submit` aufrufen.
pub async fn get_public_key(
    State(state): State<AppState>,
    Path(CodePath { code }): Path<CodePath>,
) -> Result<Json<PublicKeyResponse>, ApiError> {
    let session = require_session(&state, &code).await?;
    Ok(Json(PublicKeyResponse {
        public_key: session.public_key_b64.clone(),
    }))
}

/// `POST /{code}/submit` — Verschlüsselten Score einreichen.
///
/// Zwei Pfade:
/// - **Neuer Spieler** (player_key noch nicht in `entries`): direkt aufnehmen,
///   sofern der Raum nicht voll ist (`MAX_ENTRIES`).
/// - **Bekannter Spieler**: FHE-Maximum aus altem und neuem Score wird übernommen
///   (siehe `FheEngine::keep_max`). So „verbessert“ sich der Score nur nach oben,
///   ohne dass der Server den Klartext kennt.
///
/// Am Ende wird ein Hintergrund-Sort getriggert (Single-Flight, siehe
/// `spawn_sort_if_idle`) — der Aufrufer wartet NICHT auf das Sortier-Ergebnis,
/// die Antwort ist sofort `200 OK`.
#[tracing::instrument(skip_all, fields(code = %code))]
pub async fn submit_score(
    State(state): State<AppState>,
    Path(CodePath { code }): Path<CodePath>,
    Json(req): Json<SubmitRequest>,
) -> Result<StatusCode, ApiError> {
    let new_score = b64_decode(&req.encrypted_score).map_err(bad_req)?;
    let new_id = b64_decode(&req.encrypted_id).map_err(bad_req)?;

    let session = require_session(&state, &code).await?;

    // Eventuell vorhandenen Eintrag SUCHEN und Ciphertext CLONEN — kein Lock
    // über die anschließende FHE-Op halten. Beim Re-Submit könnte das sonst
    // mehrere Sekunden blockieren.
    let existing: Option<EncEntry> = session
        .entries
        .read()
        .await
        .iter()
        .find(|e| e.player_key == req.player_key)
        .map(|e| e.enc.clone());

    // Bei Re-Submit: FHE-Vergleich auf dem dedizierten rayon-Pool.
    // Der Pool hat den ServerKey bereits auf jedem Worker-Thread installiert.
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
        // Erste Einreichung dieses Spielers → kein Vergleich nötig.
        None => (new_score, new_id),
    };

    // Eintrag persistieren — kurzer Schreib-Lock, keine FHE-Op mehr.
    {
        let mut entries = session.entries.write().await;
        match entries.iter_mut().find(|e| e.player_key == req.player_key) {
            // Re-Submit eines bekannten Spielers: in-place überschreiben.
            Some(e) => {
                e.enc.score = kept.0;
                e.enc.id = kept.1;
            }
            // Neuer Spieler: anhängen oder mit 409 ablehnen wenn voll.
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

    // Sort im Hintergrund anstoßen — der Handler returnt sofort.
    spawn_sort_if_idle(Arc::clone(&session));
    Ok(StatusCode::OK)
}

/// `GET /{code}/entries` — Aktuelle (verschlüsselte) Leaderboard-Reihenfolge.
///
/// Bevorzugt die zuletzt fertig berechnete sortierte Sicht. Falls noch kein Sort
/// fertig ist (z.B. direkt nach dem allerersten Submit), liefern wir die
/// Insertion-Order als Fallback — besser als gar nichts.
///
/// Nur E besitzt den ClientKey und kann die Score/ID-Ciphertexts entschlüsseln.
pub async fn get_entries(
    State(state): State<AppState>,
    Path(CodePath { code }): Path<CodePath>,
) -> Result<Json<EntriesResponse>, ApiError> {
    let session = require_session(&state, &code).await?;

    let sorted = session.sorted.read().await;
    let entries: Vec<EntryDto> = if !sorted.is_empty() {
        // Happy Path: sortierte Sicht ist verfügbar.
        sorted
            .iter()
            .map(|e| EntryDto {
                encrypted_score: b64_encode(&e.score),
                encrypted_id: b64_encode(&e.id),
            })
            .collect()
    } else {
        // Fallback: noch kein Sort fertig. Read-Lock auf `sorted` früh
        // freigeben, bevor wir uns den nächsten Read-Lock auf `entries` holen
        // (verhindert Deadlock-Verdacht wenn später mal ein Writer dazwischenkommt).
        drop(sorted);
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

    // Roster aus der Quelle-der-Wahrheit `entries` (Insertion-Order), inklusive
    // Klartext-player_key. Der Creator nutzt das, um die ID-Bytes der sortierten
    // Liste auf Namen zu mappen.
    let roster: Vec<RosterEntryDto> = session
        .entries
        .read()
        .await
        .iter()
        .map(|e| RosterEntryDto {
            player_key: e.player_key.clone(),
            encrypted_id: b64_encode(&e.enc.id),
        })
        .collect();

    Ok(Json(EntriesResponse { entries, roster }))
}

/// `POST /{code}/rank` — Rang einer verschlüsselten Spieler-ID abfragen.
///
/// E sendet eine verschlüsselte ID und bekommt für jede Position der sortierten
/// Liste einen verschlüsselten Bool zurück (Treffer ja/nein). Lokale Entschlüsselung
/// bei E ergibt dann 0..n „true“-Positionen, jede entspricht einem Rang
/// (1-basiert). Funktioniert auch bei Mehrfach-Treffern, falls die ID doppelt
/// in der Liste auftaucht.
///
/// Der Service erfährt dabei NICHT, welche ID gesucht wird — die ist verschlüsselt.
#[tracing::instrument(skip_all, fields(code = %code))]
pub async fn query_rank(
    State(state): State<AppState>,
    Path(CodePath { code }): Path<CodePath>,
    Json(req): Json<RankRequest>,
) -> Result<Json<RankResponse>, ApiError> {
    let target = b64_decode(&req.encrypted_id).map_err(bad_req)?;
    let session = require_session(&state, &code).await?;

    // Snapshot der aktuellen Liste ziehen — sortiert wenn möglich, sonst
    // Insertion-Order. Read-Lock wird sofort wieder freigegeben.
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

    // Leere Liste → leere Antwort, kein FHE-Aufwand nötig.
    if snapshot.is_empty() {
        return Ok(Json(RankResponse { matches: vec![] }));
    }

    // Vergleiche laufen parallel auf dem Pool (siehe `FheEngine::rank_matches`).
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

/// Stößt einen Hintergrund-Sort an, falls noch keiner für diese Session läuft.
///
/// Wenn schon ein Sort läuft, wird stattdessen nur das `dirty`-Flag gesetzt —
/// der laufende Task zieht dann genau EINEN weiteren Pass nach, sobald er
/// fertig ist. Effekt:
/// - Ein einzelner Submit triggert genau einen Sort.
/// - Burst von 5 Submits während ein Sort läuft → höchstens „current+1“ Sorts,
///   nicht 5 hintereinander.
///
/// Das hält die CPU-Last bei vielen schnellen Submits beherrschbar, ohne dass
/// die sortierte Sicht jemals lange veraltet ist.
fn spawn_sort_if_idle(session: Arc<Session>) {
    use tracing::Instrument;
    let span = tracing::info_span!("background_sort");
    tokio::spawn(async move {
        // Sort-Slot beanspruchen — oder nur `dirty` setzen wenn bereits sortiert wird.
        {
            let mut s = session.sort_state.lock().await;
            if s.running {
                s.dirty = true;
                return;
            }
            s.running = true;
        }

        // Schleife läuft so lange, bis während eines Sort-Passes kein neuer
        // Submit mehr kam (dirty = false).
        loop {
            // Snapshot der aktuellen Einträge ziehen — Lock wird sofort wieder
            // freigegeben, der Sort läuft anschließend auf der Kopie.
            let snapshot: Vec<(Vec<u8>, Vec<u8>)> = session
                .entries
                .read()
                .await
                .iter()
                .map(|e| (e.enc.score.clone(), e.enc.id.clone()))
                .collect();

            // Trivial-Fall: 0 oder 1 Eintrag braucht keinen FHE-Sort,
            // wir kopieren einfach durch.
            if snapshot.len() <= 1 {
                let result: Vec<EncEntry> = snapshot
                    .into_iter()
                    .map(|(s, i)| EncEntry { score: s, id: i })
                    .collect();
                *session.sorted.write().await = result;
            } else {
                // Echter Sort — Batcher's Odd-Even Mergesort über FHE-Vergleichen.
                // Läuft mehrere Sekunden, daher `spawn_blocking`.
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

            // Slot freigeben oder direkt nochmal sortieren? Hängt davon ab, ob
            // während des Sorts ein neuer Submit gekommen ist.
            let mut s = session.sort_state.lock().await;
            if !s.dirty {
                s.running = false;
                return;
            }
            s.dirty = false;
        }
    }.instrument(span));
}
