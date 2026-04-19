use axum::{extract::Extension, routing::post, Json, Router};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::panic;
use std::sync::Arc;
use tfhe::prelude::*;
use tfhe::ServerKey;

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
    println!("Anfrage empfangen...");

    // ServerKey beim ersten Request laden
    let mut sk_guard = sk.0.lock().await;
    if sk_guard.is_none() {
        println!("Lade ServerKey aus Request...");
        let sk_bytes = general_purpose::STANDARD
            .decode(&req.server_key)
            .map_err(|e| format!("Base64 Decode Error: {}", e))?;

        let server_key: ServerKey =
            bincode::deserialize(&sk_bytes).map_err(|e| format!("Deserialization Error: {}", e))?;
        *sk_guard = Some(server_key);
        println!("ServerKey geladen und gespeichert");
    }

    let server_key = sk_guard.as_ref().unwrap().clone();
    drop(sk_guard); // Freigeben des Locks

    let age_bytes = general_purpose::STANDARD
        .decode(&req.encrypted_age)
        .map_err(|_| "Invalid Age Base64")?;

    println!("Deserialisiere FheUint8, Länge: {}", age_bytes.len());
    let enc_age: tfhe::FheUint8 = bincode::deserialize(&age_bytes).map_err(|e| {
        eprintln!("FheUint8 Deserialisierung fehlgeschlagen: {}", e);
        format!("Failed to deserialize Encrypted Age: {}", e)
    })?;
    println!("FheUint8 erfolgreich deserialisiert");

    println!("Starte Vergleich mit block_in_place...");

    // Fange Panics bei der TFHE-Operation
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        tokio::task::block_in_place(|| {
            println!("In block_in_place: mit_server_key_as_context wird aufgerufen...");
            tfhe::with_server_key_as_context(server_key, || {
                println!("In with_server_key_as_context: führe gt(17) aus...");
                enc_age.gt(17u8)
            })
        })
    }));

    let result = match result {
        Ok(res) => res,
        Err(e) => {
            eprintln!("TFHE Operation panicked: {:?}", e);
            return Err("TFHE Vergleichsoperation fehlgeschlagen".to_string());
        }
    };

    println!("Vergleich abgeschlossen");
    let res_bytes =
        bincode::serialize(&result).map_err(|e| format!("Serialization Error: {}", e))?;

    println!("Rückgabe: {} bytes", res_bytes.len());
    Ok(Json(AgeResponse {
        is_adult: general_purpose::STANDARD.encode(res_bytes),
    }))
}

#[tokio::main]
async fn main() {
    let sk = SharedServerKey(Arc::new(tokio::sync::Mutex::new(None)));

    let app = Router::new()
        .route("/age-verification", post(verify_age))
        .layer(Extension(sk))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 8000));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Fehler beim Binden an Port 8000: {}", e);
            eprintln!("Port ist möglicherweise noch in TIME_WAIT-Status. Warte 5 Sekunden...");
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            tokio::net::TcpListener::bind(addr)
                .await
                .expect("Konnte nach Wartezeit nicht an Port binden")
        }
    };

    println!("Server läuft auf http://127.0.0.1:8000");
    axum::serve(listener, app).await.unwrap();
}
