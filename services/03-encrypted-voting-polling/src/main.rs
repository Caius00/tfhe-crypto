mod voting;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use aide::axum::{
    routing::{get_with, post_with},
    ApiRouter,
};
use axum::extract::DefaultBodyLimit;
use tower_http::cors::{Any, CorsLayer};

use crate::voting::logic::{
    approve_participant, create_session, finalize_session, get_participants, get_results,
    get_session, get_status, join_session, submit_vote,
};
use crate::voting::types::AppState;

#[tokio::main]
async fn main() {
    // AppState ist thread-sichere Hashmap und hält die Voting-Sessions
    let state: AppState = Arc::new(Mutex::new(HashMap::new()));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let api_router = ApiRouter::new()
        .api_route(
            "/session",
            post_with(create_session, |op| {
                op.description("Create a new voting session.")
            }),
        )
        .api_route(
            "/session/{session_id}",
            get_with(get_session, |op| {
                op.description("Get questions and public key of a session.")
            }),
        )
        .api_route(
            "/join",
            post_with(join_session, |op| {
                op.description("Request to join a session (pending until approved).")
            }),
        )
        .api_route(
            "/approve",
            post_with(approve_participant, |op| {
                op.description("Approve or reject a pending participant.")
            }),
        )
        .api_route(
            "/vote",
            post_with(submit_vote, |op| {
                op.description("Submit encrypted votes for all questions.")
            }),
        )
        .api_route(
            "/status/{session_id}/{participant_id}",
            get_with(get_status, |op| {
                op.description("Poll a participant's approval status.")
            }),
        )
        .api_route(
            "/results/{session_id}/{creator_id}",
            get_with(get_results, |op| {
                op.description("Fetch homomorphically aggregated results (creator only).")
            }),
        )
        .api_route(
            "/participants/{session_id}/{creator_id}",
            get_with(get_participants, |op| {
                op.description("Get full participant status (approved + voting state).")
            }),
        )
        .api_route(
            "/finalize/{session_id}/{creator_id}",
            post_with(finalize_session, |op| {
                op.description("Close the session – no further votes accepted.")
            }),
        )
        .with_state(state);

    let app = openapi_docs::attach(
        api_router,
        "Encrypted Voting / Polling",
        "Homomorphic voting and polling service: creator-controlled sessions, \
         approval flow for participants, encrypted ballots that are aggregated \
         server-side without ever being decrypted.",
        env!("CARGO_PKG_VERSION"),
    )
    .merge(health::router(env!("CARGO_PKG_VERSION")))
    .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024))
    .layer(cors);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Fehler beim Binden an Port 8080: {}", e);
            std::process::exit(1);
        }
    };

    println!("Voting-Server läuft auf http://{}", addr);

    axum::serve(listener, app).await.unwrap();
}
