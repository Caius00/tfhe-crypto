use axum::{extract::Extension, routing::post, Json, Router};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::panic;
use std::sync::Arc;
use tfhe::prelude::FheOrd;
use tfhe::ServerKey;
use health;

#[derive(Deserialize)]
struct AgeRequest {
    encrypted_age: String,
    server_key: String,
}

#[derive(Serialize)]
struct AgeResponse {
    is_adult: String,
}

#[derive(Clone)]
struct SharedServerKey(Arc<tokio::sync::Mutex<Option<ServerKey>>>);

async fn verify_age(
    Extension(sk): Extension<SharedServerKey>,
    Json(req): Json<AgeRequest>,
) -> Result<Json<AgeResponse>, String> {
    let mut sk_guard = sk.0.lock().await;
    if sk_guard.is_none() {
        let sk_bytes = general_purpose::STANDARD
            .decode(&req.server_key)
            .map_err(|_| "Invalid ServerKey Base64")?;
        let server_key: ServerKey =
            bincode::deserialize(&sk_bytes).map_err(|_| "Failed to deserialize ServerKey")?;
        *sk_guard = Some(server_key);
    }

    let server_key = sk_guard.as_ref().unwrap().clone();
    drop(sk_guard);

    let age_bytes = general_purpose::STANDARD
        .decode(&req.encrypted_age)
        .map_err(|_| "Invalid Age Base64")?;

    let enc_age: tfhe::FheUint8 = bincode::deserialize(&age_bytes)
        .map_err(|_| "Failed to deserialize Encrypted Age")?;

    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        tokio::task::block_in_place(|| {
            tfhe::with_server_key_as_context(server_key, || {
                enc_age.ge(18u8)
            })
        })
    }));

    let enc_result: tfhe::FheBool = match result {
        Ok(res) => res,
        Err(_) => return Err("TFHE operation failed".to_string()),
    };

    let res_bytes =
        bincode::serialize(&enc_result).map_err(|_| "Serialization error")?;

    Ok(Json(AgeResponse {
        is_adult: general_purpose::STANDARD.encode(res_bytes),
    }))
}

#[tokio::main]
async fn main() {
    tokio::spawn(health::serve(8080, env!("CARGO_PKG_VERSION")));

    let sk = SharedServerKey(Arc::new(tokio::sync::Mutex::new(None)));

    let app = Router::new()
        .route("/age-verification", post(verify_age))
        .layer(Extension(sk))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 3000));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error binding to port 3000: {}", e);
            std::process::exit(1);
        }
    };

    println!("Server running on http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
