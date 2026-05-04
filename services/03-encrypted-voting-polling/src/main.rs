mod voting;

// axum ist Webframework, mit Router werden routen definiert
use axum::{
    routing::{get, post},
    Router,
};

// mittels hashmap wird die session im Arbeitsspeicher gespeichert
// Arc = atomically reference counted -> so kann State zwischen mehreren Threads geteilt werden
// mit Mutex -> schreibt nur ein Thread gleichzeitig auf State
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::voting::types::AppState;
use crate::voting::logic::{
    create_session,
    join_session,
    get_pending,
    approve_participant,
    submit_vote,
    get_results,
    get_status,
    get_session,
    finalize_session,
    store_client_key,
    load_client_key,
};


use tower_http::cors::{CorsLayer, Any};

#[tokio::main]
async fn main() {
    // AppState ist thread-sichere Hashmap und hält die Voting-Sessions
    let state: AppState = Arc::new(Mutex::new(HashMap::new()));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Session erstellen
        .route("/session", post(create_session))

        // Session-Daten abrufen (Fragen + Optionen)
        .route("/session/{session_id}", get(get_session))

        // Key Storage in Redis
        .route("/store-key", post(store_client_key))
        .route("/load-key/{session_id}", get(load_client_key))
        
        // Join
        .route("/join", post(join_session))

        // Pending Teilnehmer
        .route("/pending/{session_id}/{creator_id}", get(get_pending))

        // Approval
        .route("/approve", post(approve_participant))

        // Vote
        .route("/vote", post(submit_vote))

        // Status für Teilnehmer
        .route("/status/{session_id}/{participant_id}", get(get_status))

        // Ergebnisse
        .route("/results/{session_id}/{creator_id}", get(get_results))

        .route("/finalize/{session_id}/{creator_id}", post(finalize_session))

        .with_state(state)

        .merge(health::router(env!("CARGO_PKG_VERSION")))

        .layer(
            axum::extract::DefaultBodyLimit::max(
                2 * 1024 * 1024 * 1024
            )
        )

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

    axum::serve(listener, app)
        .await
        .unwrap();
}