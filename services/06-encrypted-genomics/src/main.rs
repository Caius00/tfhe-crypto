use aide::axum::{routing::post_with, ApiRouter};
use axum::extract::DefaultBodyLimit;
use axum::{http::StatusCode, Json};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

mod functions;

/*
#[derive(Serialize, Deserialize, JsonSchema)]
struct TestResponse {
    message: String,
}

pub(crate) async fn testfun() -> Result<Json<TestResponse>, (StatusCode, String)> {
    Ok(Json(TestResponse {
        message: "Success".to_string(),
    }))
}
*/
#[tokio::main]
async fn main() {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let shared_state = Arc::new(functions::AppState::new());

    let api_router = ApiRouter::new()
        .api_route(
            "/encrypt",
            post_with(functions::encrypt_handler, |op| {
                op.description("DNA verschlüsseln")
            }),
        )
        .api_route(
            "/process",
            post_with(functions::process_handler, |op| {
                op.description("Homomorphe Risikomuster-Prüfung")
            }),
        )
        .api_route(
            "/decrypt",
            post_with(functions::decrypt_handler, |op| {
                op.description("Ergebnisse entschlüsseln")
            }),
        )
        .with_state(shared_state);
    /*
    .api_route(
        "/",
        post_with(testfun, |op| {
            op.description("")
            .response::<200, Json<TestResponse>>()
    }))
    .api_route(
        "/test",
        get_with(testfun, |op| {
            op.description("")
            .response::<200, Json<TestResponse>>()
        })
    );
    */

    let app = openapi_docs::attach(api_router, "tammoloco", "0.1", "")
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
        .layer(cors);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Server läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

/*
use format list to see sequence original length ($enc | format-list), window length ($proc | format-list)
encrypt testsequence in body
    $enc = Invoke-RestMethod `
    -Uri "http://localhost:8080/encrypt" `
    -Method Post `
    -ContentType "application/json" `
    -Body '{"sequence":"ATCGATCG"}'
    $enc


check risk pattern , risk pattern in body
    $proc = Invoke-RestMethod `
    -Uri "http://localhost:8080/process" `
    -Method Post `
    -ContentType "application/json" `
    -Body (@{
        encrypted_sequence = $enc.encrypted_data
        risk_pattern = "012"
    } | ConvertTo-Json)

    $proc


decrypt
    $dec = Invoke-RestMethod `
    -Uri "http://localhost:8080/decrypt" `
    -Method Post `
    -ContentType "application/json" `
    -Body (@{
        encrypted_data = $proc.encrypted_distances
    } | ConvertTo-Json)

    $dec.plain_data
*/
