pub mod codec;
pub mod fhe;
pub mod handlers;
pub mod state;

use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};

use crate::state::AppState;

// Baut den vollständigen Axum-Router auf.
// Wird vom Binary mit der Paket-Version aufgerufen und kann von Tests
// mit beliebigen State-/Versions-Werten wiederverwendet werden.
pub fn app(state: AppState, version: &'static str) -> Router {
    Router::new()
        .route("/create", post(handlers::create_session))
        .route("/{code}/public-key", get(handlers::get_public_key))
        .route("/{code}/submit", post(handlers::submit_score))
        .route("/{code}/entries", get(handlers::get_entries))
        .route("/{code}/rank", post(handlers::query_rank))
        .with_state(state)
        .merge(health::router(version))
        // FHE-ServerKeys können bei /create > 100 MB groß sein
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
}
