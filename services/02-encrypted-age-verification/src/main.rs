use axum::{
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tfhe::prelude::*;
use tfhe::{FheBool, FheUint8, ServerKey};

#[derive(Deserialize)]
struct AgeRequest {
    encrypted_age: String,
    server_key: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
struct AgeResponse {
    is_adult: String,
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "UP".to_string(),
        version: env!("CARGO_PKG_VERSION"),
        service: env!("CARGO_PKG_NAME"),
    })
}

#[derive(Clone)]
struct SharedServerKey(Arc<tokio::sync::Mutex<Option<ServerKey>>>);

async fn verify_age(Json(req): Json<AgeRequest>) -> Result<Json<AgeResponse>, String> {
    let sk_bytes = general_purpose::STANDARD
        .decode(&req.server_key)
        .map_err(|_| "Invalid ServerKey Base64")?;

    let server_key: ServerKey =
        bincode::deserialize(&sk_bytes).map_err(|_| "Failed to deserialize ServerKey")?;

    let age_bytes = general_purpose::STANDARD
        .decode(&req.encrypted_age)
        .map_err(|_| "Invalid Age Base64")?;

    let enc_age: FheUint8 =
        bincode::deserialize(&age_bytes).map_err(|_| "Failed to deserialize Encrypted Age")?;

    let enc_result: FheBool = tokio::task::block_in_place(|| {
        tfhe::with_server_key_as_context(server_key, || enc_age.gt(18u8))
    });

    let res_bytes = bincode::serialize(&enc_result).map_err(|_| "Serialization error")?;

    Ok(Json(AgeResponse {
        is_adult: general_purpose::STANDARD.encode(res_bytes),
    }))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/age-verification", post(verify_age))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Fehler beim Binden an Port 3000: {}", e);
            std::process::exit(1);
        }
    };

    println!("Server läuft auf http://{}", addr);
    println!("Health Check verfügbar unter http://{}/health", addr);

    axum::serve(listener, app).await.unwrap();
}
