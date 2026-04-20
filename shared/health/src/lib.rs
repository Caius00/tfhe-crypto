use axum::{routing::get, Router};
use std::net::SocketAddr;

pub fn router(version: &'static str) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(|| async { "ok" }))
        .route("/version", get(move || async move { version }))
}

pub async fn serve(port: u16, version: &'static str) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, router(version)).await.unwrap();
}
