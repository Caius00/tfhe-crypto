mod routes;
mod store;
mod models;
mod frontend;
mod custom_fhe_ascii_string;

use tfhe::{generate_keys, set_server_key, ConfigBuilder};
use tfhe::prelude::{FheDecrypt, FheTryEncrypt, FheTryTrivialEncrypt};
use crate::custom_fhe_ascii_string::CustomFheAsciiString;
use crate::store::AppState;

// #[tokio::main]
// async fn main() {
//     // TODO() make configurable
//     let ttl_sec = 60u64;
//     // temp until api handles keys
//     let config = ConfigBuilder::default().build();
//     let (client_key, server_key) = generate_keys(config);
//     const REDIS_URL: &str = "redis://localhost:6379";
//     let state: SharedState = Arc::new(
//         AppState::new(REDIS_URL, &client_key, ttl_sec).expect("Failed to connect to Redis.")
//     );
//
//     let app = Router::new()
//         .route("/keys", post(put_entry))
//         .route("/keys", get(list_keys))
//         .route("/keys", delete(delete_all))
//         .route("/keys/{key}", get(get_entry))
//         .route("/keys/exists/{key}", get(exists))
//         .route("/keys/{key}", delete(delete_entry))
//         .with_state(state)
//         .merge(health::router(env!("CARGO_PKG_VERSION")));
//
//     let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
//     let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
//     axum::serve(listener, app).await.unwrap();
// }

#[tokio::main]
async fn main() {
    let ttl_sec = 5*60u64;
    let config = ConfigBuilder::default().build();
    let (client_key, server_key) = generate_keys(config);
    set_server_key(server_key);
    const REDIS_URL: &str = "redis://localhost:6379";
    let app_state = AppState::new(REDIS_URL, &client_key, ttl_sec).expect("Failed to connect to Redis.");

    let key = CustomFheAsciiString::new("Hello Key", &client_key);
    let value = CustomFheAsciiString::new("Hello Value", &client_key);

    app_state.put(&key, &value).await;

    println!("Getting from DB:");
    let (enc_found_value, enc_found_match) = app_state.get(&key).await.unwrap();

    println!("Decrypting Response:");
    let found_value = enc_found_value.decrypt(&client_key);
    let found_match = enc_found_match.decrypt(&client_key);

    println!("Found Value: {}, Match: {}", found_value, found_match);
}
