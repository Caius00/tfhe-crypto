mod voting;

use axum::{routing::get, Router, Json};
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use axum::http::StatusCode;
use tokio::sync::Mutex;
use voting::logic::*;
use voting::types::{SessionInfo, ApproveRequest, VoteRequest};
use base64::{Engine,engine::general_purpose::STANDARD};
use tfhe::{ConfigBuilder, generate_keys, set_server_key, FheUint8};

// Der globale State den alle Endpunkte teilen
struct AppState {
    session: Mutex<Option<voting::types::VotingSession>>,
    ready: Arc<AtomicBool>,
}

#[tokio::main]
async fn main() {
    let ready = Arc::new(AtomicBool::new(false));
    let ready_clone = ready.clone();

    // Health Service startet sofort auf 8080
    tokio::spawn(health::serve(8080, env!("CARGO_PKG_VERSION")));

    // State mit leerer Session — noch kein ServerKey
    let state = Arc::new(AppState {
        session: Mutex::new(None),
        ready: ready.clone(),
    });

    let app = Router::new()
        .route("/session", get(get_session))
        .route("/readyz", get(readyz))
        .route("/approve", axum::routing::post(approve_voter_handler))
        .route("/vote", axum::routing::post(vote_handler))
        .with_state(state.clone());

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Voting Server läuft auf http://localhost:3000/session");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();

    // Server in eigenem Task
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Keys generieren ohne den Tokio Runtime zu blockieren
    println!("Keys generieren...");
    let (client_key, server_key) = tokio::task::spawn_blocking(|| {
        let config = ConfigBuilder::default().build();
        generate_keys(config)
    }).await.unwrap();
    set_server_key(server_key);

    // Jetzt erst Session erstellen (ServerKey ist gesetzt)
    let session = create_session(1, "Lieblingssprache?", vec!["Rust", "Go", "Python"]);
    *state.session.lock().await = Some(session);

    // Jetzt ist alles bereit
    ready_clone.store(true, Ordering::SeqCst);
    println!("Bereit.");

    tokio::signal::ctrl_c().await.unwrap();
}


async fn readyz(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> axum::http::StatusCode {
    if state.ready.load(Ordering::SeqCst) {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    }
}

// Handler für GET /session
async fn get_session(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> Json<Option<SessionInfo>> {
    let session = state.session.lock().await;

    match session.as_ref() {
        None => Json(None),
        Some(s) => Json(Some(SessionInfo {
            id: s.id,
            question: s.question.clone(),
            options: s.options.clone(),
            vote_count: s.votes_cast.len(),
            approved_voters: s.approved_voters.clone(),
        })),
    }
}

async fn approve_voter_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(body): Json<ApproveRequest>,
) -> StatusCode {
    let mut session = state.session.lock().await;

    match session.as_mut() {
        None => StatusCode::SERVICE_UNAVAILABLE,
        Some(s) => {
            approve_voter(s, &body.voter_id);
            println!("Voter '{}' zugelassen.", body.voter_id);
            StatusCode::OK
        }
    }
}

async fn vote_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    Json(body): Json<VoteRequest>,
) -> StatusCode {
    let mut session = state.session.lock().await;

    match session.as_mut() {
        None => StatusCode::SERVICE_UNAVAILABLE,
        Some(s) => {
            // Prüfung ob Voter zugelassen
            if !s.approved_voters.contains(&body.voter_id) {
                println!("Voter '{}' nicht zugelassen.", body.voter_id);
                return StatusCode::FORBIDDEN;
            }
            // Prüfung ob bereits abgestimmt
            if s.votes_cast.contains(&body.voter_id) {
                println!("Voter '{}' hat bereits abgestimmt.", body.voter_id);
                return StatusCode::CONFLICT;
            }
            // Anzahl der Optionen prüfen
            if body.choices.len() != s.options.len(){
                return StatusCode::BAD_REQUEST;
            }

            // Base64 -> FheUint8 deserilasieren
            let choices: Result<Vec<FheUint8>, _> = body.choices.iter().map(|c| {
                let bytes = STANDARD.decode(c).map_err(|_| ())?;
                bincode::deserialize::<FheUint8>(&bytes).map_err(|_| ())
            }).collect();

            match choices {
                Err(_) => StatusCode::BAD_REQUEST,
                Ok(choices) => {
                    // verschlüsselt addieren
                    for (tally,choice) in s.tallies.iter_mut().zip(choices.iter()) {
                        *tally = &*tally + choice;
                    }
                    s.votes_cast.push(body.voter_id.clone());
                    println!("Voter '{}' hat abgestimmt.", body.voter_id);
                    StatusCode::OK
                }
            }

        }
    }
}

