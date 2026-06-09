//! # Encrypted Statistics Service
//!
//! Berechnet Summe, Anzahl, Min, Max, Durchschnitt und Median über eine verschlüsselte
//! Ganzzahlen-Liste, ohne die Werte jemals im Klartext zu sehen. Client und Server tauschen
//! ausschließlich FHE-Ciphertexte aus (Base64/bincode-kodiert).
//!
//! ## Session-basiertes Key-Caching
//!
//! Der ~80 MB große ServerKey wird einmalig via `POST /session` hochgeladen und einer
//! UUID zugewiesen. Folgende Berechnungsrequests (`POST /`) senden nur die UUID —
//! kein Key-Overhead mehr pro Request.
//!
//! Workflow:
//! 1. `POST /session { server_key }` → `{ session_id: "uuid-v4" }`
//! 2. `POST / { session_id, encrypted_list, bit_width }` → `StatisticsResponse`
//!
//! ## Typen-Mapping je nach `bit_width`
//!
//! | bit_width | Eingabe  | Summe / Durchschnitt |
//! |-----------|----------|----------------------|
//! | 8         | FheInt8  | FheInt16             |
//! | 16        | FheInt16 | FheInt32             |
//! | 32        | FheInt32 | FheInt64             |

mod fhe;
mod state;
mod statistics;

#[cfg(test)]
mod statistics_tests;

use crate::state::{AppState, Session, JANITOR_INTERVAL, SESSION_IDLE_TIMEOUT};
use crate::statistics::DivideByElementCount;
use aide::axum::{routing::post_with, ApiRouter};
use axum::{extract::State, http::StatusCode, Json, Router};
use base64::{engine::general_purpose, Engine as _};
use schemars::JsonSchema;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::ops::Add;
use std::sync::Arc;
use tfhe::prelude::{CastInto, FheOrd, IfThenElse};
use tfhe::{CompressedServerKey, FheBool, FheInt16, FheInt32, FheInt64, FheInt8};

// ── Request / Response DTOs ──────────────────────────────────────────────────

/// Anfrage zum Anlegen einer neuen Session.
/// Der ServerKey wird einmalig hochgeladen — alle folgenden Berechnungsrequests
/// nutzen nur noch die zurückgegebene `session_id`.
#[derive(Deserialize, Serialize, JsonSchema)]
struct CreateSessionRequest {
    /// Base64-kodierter, bincode-serialisierter CompressedServerKey.
    server_key: String,
}

/// Antwort auf `POST /session`.
#[derive(Serialize, Deserialize, JsonSchema)]
struct CreateSessionResponse {
    /// UUID-v4, die den hochgeladenen ServerKey identifiziert.
    /// In allen folgenden Berechnungsrequests als `session_id` mitschicken.
    session_id: String,
}

/// Anfrage des Clients an den Statistics-Endpunkt.
#[derive(Deserialize, Serialize, JsonSchema)]
struct StatisticsRequest {
    /// UUID aus `POST /session` — identifiziert den vorab hochgeladenen ServerKey.
    session_id: String,
    /// Jedes Element ist ein Base64-kodiertes, bincode-serialisiertes FHE-Integer.
    /// Der konkrete Typ richtet sich nach `bit_width`.
    encrypted_list: Vec<String>,
    /// Bitbreite der verschlüsselten Eingabewerte: 8, 16 oder 32.
    /// Wird vom Client automatisch anhand des Wertebereichs der Eingabe gewählt.
    bit_width: u8,
}

/// Antwort des Servers an den Client.
/// sum/average haben den nächstbreiteren Typ als die Eingabe (Overflow-Schutz).
/// Alle Felder sind Base64-kodierte, bincode-serialisierte FHE-Ciphertexte.
#[derive(Serialize, Deserialize, JsonSchema)]
struct StatisticsResponse {
    /// FHE-Integer mit doppelter Eingabe-Bitbreite (z.B. Int8-Eingabe → Int16-Summe)
    sum: String,
    /// Klartextzahl — die Listenlänge ist dem Server bereits aus dem Request bekannt
    count: u64,
    /// Gleicher Typ wie die Eingabe
    min: String,
    /// Gleicher Typ wie die Eingabe
    max: String,
    /// FHE-Integer mit doppelter Eingabe-Bitbreite (Overflow-Schutz, siehe sum)
    average: String,
    /// Gleicher Typ wie die Eingabe (Lower Median bei gerader Länge)
    median: String,
    /// Tatsächlich verwendete Bitbreite: 8, 16 oder 32.
    bit_width: u8,
}

// ── Hilfsfunktionen ──────────────────────────────────────────────────────────

/// Deserialisiert eine Liste von Base64-kodierten FHE-Ciphertexten in den konkreten Typ T.
fn deserialize_encrypted_list<T: DeserializeOwned>(
    base64_encoded_list: &[String],
) -> Result<Vec<T>, (StatusCode, String)> {
    base64_encoded_list
        .iter()
        .map(|base64_item| {
            let raw_bytes =
                general_purpose::STANDARD
                    .decode(base64_item)
                    .map_err(|e| (StatusCode::BAD_REQUEST, format!("Ungültiger Item-Base64: {e}")))?;
            bincode::deserialize(&raw_bytes).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("Fehler beim Deserialisieren des Ciphertexts: {e}"),
                )
            })
        })
        .collect()
}

/// Führt alle homomorphen Berechnungen für eine typisierte verschlüsselte Liste durch.
fn compute_statistics_typed<InputType, WiderOutputType>(
    encrypted_input_list: Vec<InputType>,
    engine: Arc<fhe::FheEngine>,
    element_count: u64,
    bit_width: u8,
) -> Result<Json<StatisticsResponse>, (StatusCode, String)>
where
    InputType: Clone + FheOrd + CastInto<WiderOutputType> + Sync + Send + Serialize,
    WiderOutputType: Add<WiderOutputType, Output = WiderOutputType>
        + DivideByElementCount
        + Send
        + Serialize
        + Clone,
    FheBool: IfThenElse<InputType>,
{
    let (encrypted_sum, encrypted_min, encrypted_max, encrypted_average, encrypted_median) =
        tokio::task::block_in_place(|| {
            engine.install(|| {
                let encrypted_sum: WiderOutputType = statistics::sum(&encrypted_input_list);
                let encrypted_min: InputType = statistics::min(&encrypted_input_list);
                let encrypted_max: InputType = statistics::max(&encrypted_input_list);
                let encrypted_average: WiderOutputType =
                    statistics::average_from_sum(encrypted_sum.clone(), element_count as usize);
                let encrypted_median: InputType = statistics::median(&encrypted_input_list);
                (
                    encrypted_sum,
                    encrypted_min,
                    encrypted_max,
                    encrypted_average,
                    encrypted_median,
                )
            })
        });

    Ok(Json(StatisticsResponse {
        sum: to_base64(&encrypted_sum)?,
        count: element_count,
        min: to_base64(&encrypted_min)?,
        max: to_base64(&encrypted_max)?,
        average: to_base64(&encrypted_average)?,
        median: to_base64(&encrypted_median)?,
        bit_width,
    }))
}

/// Serialisiert einen FHE-Ciphertext via bincode und kodiert ihn als Base64-String.
fn to_base64<T: Serialize>(value: &T) -> Result<String, (StatusCode, String)> {
    bincode::serialize(value)
        .map(|bytes| general_purpose::STANDARD.encode(bytes))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Serialisierungsfehler: {e}")))
}

// ── Handler ──────────────────────────────────────────────────────────────────

/// `POST /session` — ServerKey einmalig hochladen, UUID erhalten.
///
/// Der CompressedServerKey wird dekomprimiert und in einer `FheEngine`
/// (dedizierter Rayon-Pool) gespeichert. Die UUID identifiziert diese Session
/// in allen folgenden Berechnungsrequests.
#[tracing::instrument(skip_all)]
async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<Json<CreateSessionResponse>, (StatusCode, String)> {
    let server_key_bytes = general_purpose::STANDARD
        .decode(&request.server_key)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Ungültiger ServerKey Base64: {e}")))?;

    let engine = tokio::task::spawn_blocking(move || -> Result<fhe::FheEngine, String> {
        let compressed: CompressedServerKey = bincode::deserialize(&server_key_bytes)
            .map_err(|e| format!("Fehler beim Deserialisieren des ServerKey: {e}"))?;
        fhe::FheEngine::from_server_key(compressed.decompress())
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("Task-Fehler: {e}")))?
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;

    let session = Arc::new(Session::new(Arc::new(engine)));
    let session_id = state.insert(session).await;

    tracing::info!(%session_id, "statistics session created");
    Ok(Json(CreateSessionResponse { session_id }))
}

/// `POST /` — Statistiken homomorph berechnen.
///
/// Erwartet eine `session_id` aus `POST /session`. Die FHE-Engine der Session
/// wird wiederverwendet — kein Key-Overhead pro Request.
#[tracing::instrument(skip_all, fields(session_id = %request.session_id))]
async fn compute_statistics(
    State(state): State<AppState>,
    Json(request): Json<StatisticsRequest>,
) -> Result<Json<StatisticsResponse>, (StatusCode, String)> {
    let session = state
        .get(&request.session_id)
        .await
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Session nicht gefunden oder abgelaufen".into()))?;

    if request.encrypted_list.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Die Liste darf nicht leer sein".into()));
    }

    let element_count = request.encrypted_list.len() as u64;
    let engine = Arc::clone(&session.engine);

    match request.bit_width {
        8 => {
            let list = deserialize_encrypted_list::<FheInt8>(&request.encrypted_list)?;
            compute_statistics_typed::<FheInt8, FheInt16>(list, engine, element_count, 8)
        }
        16 => {
            let list = deserialize_encrypted_list::<FheInt16>(&request.encrypted_list)?;
            compute_statistics_typed::<FheInt16, FheInt32>(list, engine, element_count, 16)
        }
        32 => {
            let list = deserialize_encrypted_list::<FheInt32>(&request.encrypted_list)?;
            compute_statistics_typed::<FheInt32, FheInt64>(list, engine, element_count, 32)
        }
        bw => Err((
            StatusCode::BAD_REQUEST,
            format!("Ungültige Bitbreite {bw}: muss 8, 16 oder 32 sein."),
        )),
    }
}

// ── Router ───────────────────────────────────────────────────────────────────

/// Baut den vollständigen Axum-Router zusammen.
pub(crate) fn create_app(state: AppState) -> Router {
    let (metrics_layer, metrics_router) = metrics_exporter::setup();

    let api_router = ApiRouter::new()
        .api_route(
            "/session",
            post_with(create_session, |op| {
                op.description(
                    "Upload the ServerKey once and receive a session UUID. \
                     Use the UUID in subsequent compute requests instead of re-sending the key.",
                )
                .response::<200, Json<CreateSessionResponse>>()
            }),
        )
        .api_route(
            "/",
            post_with(compute_statistics, |op| {
                op.description(
                    "Compute sum, count, min, max, average and median homomorphically \
                     on an encrypted integer list. Requires a session_id from POST /session.",
                )
                .response::<200, Json<StatisticsResponse>>()
            }),
        )
        .with_state(state);

    openapi_docs::attach(
        api_router,
        "Encrypted Statistics Service",
        "Homomorphic statistics service: computes sum, count, min, max, average and median \
         on an encrypted integer list — the server never sees the values. \
         Use POST /session to upload the ServerKey once, then POST / with the session_id.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .merge(metrics_router)
    // 2 GB Limit: POST /session trägt den ~80 MB CompressedServerKey.
    .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
    .layer(metrics_layer)
    .layer(observability::http_trace_layer())
}

#[tokio::main]
async fn main() {
    observability::init("encrypted-statistics-service", env!("CARGO_PKG_VERSION"));

    let state = AppState::new();
    state.spawn_janitor(SESSION_IDLE_TIMEOUT, JANITOR_INTERVAL);

    let listening_address = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let tcp_listener = tokio::net::TcpListener::bind(listening_address)
        .await
        .unwrap();
    println!("Statistics Service läuft auf http://{}", listening_address);
    axum::serve(tcp_listener, create_app(state)).await.unwrap();

    observability::shutdown();
}
