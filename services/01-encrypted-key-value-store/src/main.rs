mod routes;
mod store;
mod models;
mod custom_fhe_ascii_string;

use std::env;
use std::sync::Arc;
use axum::Router;
use axum::routing::{delete, get, post};
use tfhe::prelude::{FheDecrypt};
use crate::routes::{delete_route, exists_route, get_route, put_route};
use crate::store::{AppState, SharedState};

#[tokio::main]
async fn main() {
    let state: SharedState = Arc::new(
        AppState::new()
    );

    let app = Router::new()
        .route("/entry", post(put_route))
        .route("/entry", get(get_route))
        .route("/entry/exists", get(exists_route))
        .route("/entry", delete(delete_route))
        .route("/entries", delete(delete_route))
        .with_state(state)
        .merge(health::router(env!("CARGO_PKG_VERSION")));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// #[tokio::main]
// async fn main() {
//     let config = ConfigBuilder::default().build();
//     let (client_key, server_key) = generate_keys(config);
//
//     let key = CustomFheAsciiString::new("Hello Key", &client_key);
//     let value = CustomFheAsciiString::new("Hello Value", &client_key);
//
//     // SERVER
//     set_server_key(server_key);
//     let app_state = AppState::new();
//
//     app_state.put(&key, &value).await;
//
//     println!("Getting from DB:");
//     let (enc_found_value, enc_found_match) = app_state.get(&key).await.unwrap();
//
//     println!("Decrypting Response:");
//     let found_value = enc_found_value.decrypt(&client_key);
//     let found_match = enc_found_match.decrypt(&client_key);
//
//     println!("Found Value: {}, Match: {}", found_value, found_match);
// }
