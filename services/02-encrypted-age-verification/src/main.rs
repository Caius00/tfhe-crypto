#[cfg(test)]
mod age_verification_tests;

use aide::axum::{
    routing::{delete_with, post_with},
    ApiRouter,
};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheBool, FheInt8, ServerKey};
use tokio::sync::RwLock;
use uuid::Uuid;

type SessionStore = Arc<RwLock<HashMap<String, Arc<ServerKey>>>>;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) sessions: SessionStore,
}

#[derive(Serialize, Deserialize, JsonSchema)]
pub(crate) struct AgeResponse {
    /// Base64-kodierter `FheBool` — true wenn Alter ≥ 18 und ≥ 0.
    pub(crate) is_adult: String,
}

#[derive(Deserialize, Serialize, JsonSchema)]
struct SetupRequest {
    /// Base64-kodierter `CompressedServerKey` (bincode-serialisiert).
    server_key: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct SetupResponse {
    /// UUID der neu erstellten Session.
    session_id: String,
}

/// Request für den session-basierten Endpunkt – kein server_key mehr nötig.
#[derive(Deserialize, Serialize, JsonSchema)]
struct SessionAgeRequest {
    /// Base64-kodierter, mit dem ClientKey verschlüsselter `FheInt8` (Alter in Jahren).
    encrypted_age: String,
}

pub(crate) fn decode_server_key(
    encoded: &str,
) -> Result<CompressedServerKey, (StatusCode, String)> {
    let bytes = general_purpose::STANDARD.decode(encoded).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Ungültiger ServerKey (Base64): {}", e),
        )
    })?;
    bincode::deserialize(&bytes).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!(
                "Deserialisierung von CompressedServerKey fehlgeschlagen: {}",
                e
            ),
        )
    })
}

pub(crate) fn decode_encrypted_age(encoded: &str) -> Result<FheInt8, (StatusCode, String)> {
    let bytes = general_purpose::STANDARD.decode(encoded).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Ungültiger Age (Base64): {}", e),
        )
    })?;
    bincode::deserialize(&bytes).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Deserialisierung von Encrypted Age fehlgeschlagen: {}", e),
        )
    })
}

pub(crate) fn age_check(enc_age: &FheInt8) -> FheBool {
    let is_adult = enc_age.gt(17i8);
    let is_positive = enc_age.ge(0i8);
    is_adult & is_positive
}

pub(crate) fn encode_result(result: &FheBool) -> Result<String, (StatusCode, String)> {
    bincode::serialize(result)
        .map(|bytes| general_purpose::STANDARD.encode(bytes))
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Serialization error: {}", e),
            )
        })
}

/// POST /session
/// Lädt den CompressedServerKey einmalig hoch, dekomprimiert ihn und gibt
/// eine session_id zurück. Alle folgenden /verify/:id Requests sind ~88 KB.
async fn setup_session(
    State(state): State<AppState>,
    Json(req): Json<SetupRequest>,
) -> Result<Json<SetupResponse>, (StatusCode, String)> {
    let compressed = decode_server_key(&req.server_key)?;
    let server_key = tokio::task::block_in_place(|| compressed.decompress());

    let session_id = Uuid::new_v4().to_string();
    state
        .sessions
        .write()
        .await
        .insert(session_id.clone(), Arc::new(server_key));

    Ok(Json(SetupResponse { session_id }))
}

/// POST /verify/:session_id
/// Nutzt den gecachten ServerKey – kein server_key im Request-Body.
async fn verify_age_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(req): Json<SessionAgeRequest>,
) -> Result<Json<AgeResponse>, (StatusCode, String)> {
    let server_key = {
        let sessions = state.sessions.read().await;
        sessions.get(&session_id).cloned().ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("Session '{}' nicht gefunden", session_id),
            )
        })?
    };

    let enc_age = decode_encrypted_age(&req.encrypted_age)?;

    let enc_result = tokio::task::block_in_place(|| {
        tfhe::set_server_key((*server_key).clone());
        age_check(&enc_age)
    });

    Ok(Json(AgeResponse {
        is_adult: encode_result(&enc_result)?,
    }))
}

/// DELETE /session/:session_id
/// Löscht die Session und gibt den RAM frei.
async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    match state.sessions.write().await.remove(&session_id) {
        Some(_) => Ok(Json(serde_json::json!({ "status": "deleted" }))),
        None => Err((
            StatusCode::NOT_FOUND,
            format!("Session '{}' nicht gefunden", session_id),
        )),
    }
}

//App

pub fn create_app() -> Router {
    let state = AppState {
        sessions: Arc::new(RwLock::new(HashMap::new())),
    };

    let api_router = ApiRouter::new()
        .api_route(
            "/session",
            post_with(setup_session, |op| {
                op.description("Erstellt eine Session mit gecachtem ServerKey.")
                    .response::<200, Json<SetupResponse>>()
            }),
        )
        .api_route(
            "/verify/{session_id}",
            post_with(verify_age_session, |op| {
                op.description("Session-basierte Altersverifikation.")
                    .response::<200, Json<AgeResponse>>()
            }),
        )
        .api_route(
            "/session/{session_id}",
            delete_with(delete_session, |op| {
                op.description("Löscht eine Session.")
                    .response::<200, Json<serde_json::Value>>()
            }),
        )
        .with_state(state);

    openapi_docs::attach(
        api_router,
        "Encrypted Age Verification",
        "Homomorphic age-check service.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
}

#[tokio::main]
async fn main() {
    let app = create_app();
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
