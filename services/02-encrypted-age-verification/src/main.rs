#[cfg(test)]
mod age_verification_integration_test;
use axum::{routing::post, Json, Router};
use base64::{engine::general_purpose, Engine as _};
use health;
use serde::{Deserialize, Serialize};
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheBool, FheInt8};

#[derive(Deserialize, Serialize)]
struct AgeRequest {
    encrypted_age: String,
    server_key: String,
}

#[derive(Serialize, Deserialize)]
struct AgeResponse {
    is_adult: String,
}

pub(crate) async fn verify_age(Json(req): Json<AgeRequest>) -> Result<Json<AgeResponse>, String> {
    let sk_bytes = general_purpose::STANDARD
        .decode(&req.server_key)
        .map_err(|e| format!("Invalid ServerKey Base64: {}", e))?;

    let compressed: CompressedServerKey = bincode::deserialize(&sk_bytes)
        .map_err(|e| format!("Failed to deserialize CompressedServerKey: {}", e))?;

    let server_key = compressed.decompress();

    let age_bytes = general_purpose::STANDARD
        .decode(&req.encrypted_age)
        .map_err(|e| format!("Invalid Age Base64: {}", e))?;

    let enc_age: FheInt8 = bincode::deserialize(&age_bytes)
        .map_err(|e| format!("Failed to deserialize Encrypted Age: {}", e))?;

    let enc_result: FheBool = tokio::task::block_in_place(|| {
        tfhe::set_server_key(server_key);

        let is_adult = enc_age.gt(17i8);
        let is_positive = enc_age.ge(0i8);

        is_adult & is_positive
    });

    let res_bytes =
        bincode::serialize(&enc_result).map_err(|e| format!("Serialization error: {}", e))?;

    Ok(Json(AgeResponse {
        is_adult: general_purpose::STANDARD.encode(res_bytes),
    }))
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", post(verify_age))
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Fehler beim Binden an Port 3000: {}", e);
            std::process::exit(1);
        }
    };

    println!("Server läuft auf http://{}", addr);

    axum::serve(listener, app).await.unwrap();
}
