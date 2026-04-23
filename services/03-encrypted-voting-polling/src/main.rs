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
};
use tower_http::cors::{CorsLayer, Any};


#[tokio::main]
async fn main() {
    //AppState ist thread-sichere Hashmap und hält die Voting-Sessions
    let state: AppState = Arc::new(Mutex::new(HashMap::new()));

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    
    let app = Router::new()
        .route("/session", post(create_session))
        .route("/join", post(join_session))
        .route("/pending/{session_id}/{creator_id}", get(get_pending))
        .route("/approve", post(approve_participant))
        .route("/vote", post(submit_vote))
        .route("/results/{session_id}/{creator_id}", get(get_results))
        .with_state(state)
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)) // Request-Body darf max. 2 GB groß sein
        .layer(cors);
    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));

    // server lauscht auf Port 8080 auf allen Interfaces(0.0.0.0)
    // tokio -> asynchrone Runtime
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