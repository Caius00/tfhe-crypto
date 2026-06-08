#[cfg(test)]
mod age_verification_tests;

use aide::axum::{routing::post_with, ApiRouter};
use axum::{extract::DefaultBodyLimit, http::StatusCode, Json, Router};
use base64::{engine::general_purpose, Engine as _};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheBool, FheInt8};

#[derive(Deserialize, Serialize, JsonSchema)]
struct AgeRequest {
    /// Base64-kodierter, mit dem ClientKey verschlüsselter `FheInt8` (Alter in Jahren).
    encrypted_age: String,
    /// Base64-kodierter `CompressedServerKey` (bincode-serialisiert).
    server_key: String,
}

#[derive(Serialize, Deserialize, JsonSchema)]
struct AgeResponse {
    /// Base64-kodierter `FheBool` — true wenn Alter ≥ 18 und ≥ 0.
    is_adult: String,
}

pub fn create_app() -> Router {
    let api_router = ApiRouter::new().api_route(
        "/",
        post_with(verify_age, |op| {
            op.description("Encrypted age verification — returns an encrypted boolean.")
                .response::<200, Json<AgeResponse>>()
        }),
    );

    openapi_docs::attach(
        api_router,
        "Encrypted Age Verification",
        "Homomorphic age-check service: compares an encrypted age against the \
         hardcoded threshold of 18 and returns an encrypted boolean.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
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

pub(crate) async fn verify_age(
    Json(req): Json<AgeRequest>,
) -> Result<Json<AgeResponse>, (StatusCode, String)> {
    let compressed = decode_server_key(&req.server_key)?;
    let enc_age = decode_encrypted_age(&req.encrypted_age)?;

    let enc_result = tokio::task::block_in_place(|| {
        tfhe::set_server_key(compressed.decompress());
        age_check(&enc_age)
    });

    Ok(Json(AgeResponse {
        is_adult: encode_result(&enc_result)?,
    }))
}

#[tokio::main]
async fn main() {
    let app = create_app();
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}
