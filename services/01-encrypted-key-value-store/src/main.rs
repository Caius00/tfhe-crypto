mod custom_fhe_ascii_string;
mod models;
mod routes;
mod store;

use crate::routes::{
    clear_db, create_session_route, delete_route, exists_route, get_route, put_route,
};
use crate::store::{AppState, SharedState};
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use std::env;
use std::sync::Arc;
use aide::axum::ApiRouter;

#[tokio::main]
async fn main() {
    let state: SharedState = Arc::new(AppState::new());

    // TODO() set fixed length for keys/ values
    // TODO() check if works
    // TODO() use threadpool and add parallelisation (does it work?)
    // TODO() change from u8 to bigger number to store more with less overhead

    let api_router = ApiRouter::new()
        .route("/session", post(create_session_route))
        .route("/entry", post(put_route)) // TODO() better naming for put_route (not http put, but key-value put meant)
        .route("/entry", get(get_route))
        .route("/entry/exists", get(exists_route))
        .route("/entry", delete(delete_route))
        .route("/clear", delete(clear_db))
        .with_state(state);

    let app = openapi_docs::attach(
        api_router,
        "Key Value Store",
        "Stores both keys and values in its homomorphic representation.\
        Both keys and values are stored in an array of FheUint8.",
        env!("CARGO_PKG_VERSION"),
    )
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
