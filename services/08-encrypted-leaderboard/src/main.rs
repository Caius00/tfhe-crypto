mod codec;
mod fhe;
mod handlers;
mod state;

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};

use crate::state::AppState;

#[tokio::main]
async fn main() {
    let state = AppState::new();

    let app = Router::new()
        .route("/create", post(handlers::create_session))
        .route("/{code}/public-key", get(handlers::get_public_key))
        .route("/{code}/submit", post(handlers::submit_score))
        .route("/{code}/entries", get(handlers::get_entries))
        .route("/{code}/rank", post(handlers::query_rank))
        .with_state(state)
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        // FHE-ServerKeys werden bei /create hochgeladen und können > 100 MB groß sein
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("Leaderboard service on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
