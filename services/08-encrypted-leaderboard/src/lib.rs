pub mod codec;
pub mod fhe;
pub mod handlers;
pub mod state;

use aide::axum::{
    routing::{get_with, post_with},
    ApiRouter,
};
use axum::{extract::DefaultBodyLimit, Router};

use crate::state::AppState;

// Baut den vollständigen Axum-Router auf.
// Wird vom Binary mit der Paket-Version aufgerufen und kann von Tests
// mit beliebigen State-/Versions-Werten wiederverwendet werden.
pub fn app(state: AppState, version: &'static str) -> Router {
    let (metrics_layer, metrics_router) = metrics_exporter::setup();

    let api_router = ApiRouter::new()
        .api_route(
            "/create",
            post_with(handlers::create_session, |op| {
                op.description("Create a new leaderboard room. Returns a 6-digit code.")
            }),
        )
        .api_route(
            "/{code}/public-key",
            get_with(handlers::get_public_key, |op| {
                op.description("Get the room's public key so players can encrypt scores.")
            }),
        )
        .api_route(
            "/{code}/submit",
            post_with(handlers::submit_score, |op| {
                op.description("Submit an encrypted score. Re-submits keep the FHE-max.")
            }),
        )
        .api_route(
            "/{code}/entries",
            get_with(handlers::get_entries, |op| {
                op.description("List entries — sorted view if ready, otherwise insertion order.")
            }),
        )
        .api_route(
            "/{code}/rank",
            post_with(handlers::query_rank, |op| {
                op.description("Rank lookup: per-position encrypted match bools.")
            }),
        )
        .with_state(state);

    openapi_docs::attach(
        api_router,
        "Encrypted Leaderboard",
        "Homomorphic leaderboard service: encrypted score submission, FHE-sorted \
         ranking, and encrypted rank queries — only E (the creator) can decrypt.",
        version,
    )
    .merge(health::router(version))
    .merge(metrics_router)
    // FHE-ServerKeys können bei /create > 100 MB groß sein
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
    .layer(metrics_layer)
    .layer(observability::http_trace_layer())
}
