use axum::{extract::DefaultBodyLimit, routing::post, Json, Router};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use tfhe::prelude::*;
use tfhe::{FheUint8, ServerKey};

#[derive(Deserialize)]
struct AgeRequest {
    encrypted_age: String,
    server_key: String,
}

#[derive(Serialize)]
struct AgeResponse {
    is_adult: String,
}

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

    let result = tfhe::with_server_key_as_context(server_key, || enc_age.ge(18u8));

    let res_bytes = bincode::serialize(&result).unwrap();

    Ok(Json(AgeResponse {
        is_adult: general_purpose::STANDARD.encode(res_bytes),
    }))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/age-verification", post(verify_age))
        .layer(DefaultBodyLimit::max(1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
