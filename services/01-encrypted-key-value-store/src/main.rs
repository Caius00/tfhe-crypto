mod routes;
mod store;
mod models;
mod custom_fhe_ascii_string;

use std::env;
use std::sync::Arc;
use axum::extract::DefaultBodyLimit;
use axum::Router;
use axum::routing::{delete, get, post};
use crate::routes::{create_session_route, delete_route, exists_route, get_route, put_route};
use crate::store::{AppState, SharedState};

#[tokio::main]
async fn main() {
    let state: SharedState = Arc::new(
        AppState::new()
    );

    let app = Router::new() // TODO() change to aide::axum::routing::ApiRouter
        .route("/session", post(create_session_route))
        .route("/entry", post(put_route)) // TODO() better naming for put_route (not http put, but key-value put meant)
        .route("/entry", get(get_route))
        .route("/entry/exists", get(exists_route))
        .route("/entry", delete(delete_route))
        .with_state(state)
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // TODO() this is huge; find better solution
        .merge(health::router(env!("CARGO_PKG_VERSION")));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
