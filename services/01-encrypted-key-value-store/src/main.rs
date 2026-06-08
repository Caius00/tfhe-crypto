mod custom_fhe_ascii_string;
mod models;
mod routes;
mod store;

use crate::routes::{
    clear_db, create_session_route, delete_route, exists_route, get_route, put_route,
};
use crate::store::{AppState, SharedState};
use aide::axum::routing::{delete_with, get_with, post_with};
use aide::axum::ApiRouter;
use axum::extract::DefaultBodyLimit;
use std::env;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let state: SharedState = Arc::new(AppState::new());

    // TODO() set fixed length for keys/ values
    // TODO() check if works
    // TODO() use threadpool and add parallelisation (does it work?)
    // TODO() change from u8 to bigger number to store more with less overhead

    let api_router = ApiRouter::new()
        .api_route(
            "/session",
            post_with(create_session_route, |op| {
                op.description(
                    "Upload a CompressedServerKey (bincode bytes) and receive a session id.",
                )
            }),
        )
        .api_route(
            "/entry",
            // TODO() better naming for put_route (not http put, but key-value put meant)
            post_with(put_route, |op| {
                op.description("Store an encrypted key/value pair under the given session.")
            })
            .get_with(get_route, |op| {
                op.description("Fetch the encrypted value for an encrypted key under the session.")
            })
            .delete_with(delete_route, |op| {
                op.description("Delete an encrypted entry. Prefer letting the TTL expire instead.")
            }),
        )
        .api_route(
            "/entry/exists",
            get_with(exists_route, |op| {
                op.description("Return an encrypted FheBool indicating whether the key exists.")
            }),
        )
        .api_route(
            "/clear",
            delete_with(clear_db, |op| {
                op.description("Flush the entire Redis database. Destructive — no auth.")
            }),
        )
        .with_state(state);

    let app = openapi_docs::attach(
        api_router,
        "Encrypted Key-Value Store",
        "Homomorphic key-value store backed by Redis. Clients open a session by \
         uploading a CompressedServerKey, then put/get/exists/delete entries whose \
         keys and values are FHE-encrypted ASCII strings. Entries expire after TTL_MINUTES.",
        env!("CARGO_PKG_VERSION"),
    )
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // TODO() this is huge; find better solution
    .merge(health::router(env!("CARGO_PKG_VERSION")));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
