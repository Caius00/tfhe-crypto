mod routes;
mod store;
mod models;

use std::sync::Arc;
use axum::Router;
use axum::routing::{delete, get, post};
use tfhe::{generate_keys, ConfigBuilder};
use crate::routes::{put_entry, get_entry, delete_entry, list_keys, exists, delete_all};
use store::SharedState;
use crate::store::AppState;

#[tokio::main]
async fn main() {
    // TODO() make configurable
    let ttl_sec = 60u64;
    // temp until api handles keys
    let config = ConfigBuilder::default().build();
    let (client_key, server_key) = generate_keys(config);
    const REDIS_URL: &str = "redis://localhost:6379";
    let state: SharedState = Arc::new(
        AppState::new(REDIS_URL, &client_key, ttl_sec).expect("Failed to connect to Redis.")
    );

    let app = Router::new()
        .route("/keys", post(put_entry))
        .route("/keys", get(list_keys))
        .route("/keys", delete(delete_all))
        .route("/keys/{key}", get(get_entry))
        .route("/keys/exists/{key}", get(exists))
        .route("/keys/{key}", delete(delete_entry))
        .with_state(state)
        .merge(health::router(env!("CARGO_PKG_VERSION")));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
