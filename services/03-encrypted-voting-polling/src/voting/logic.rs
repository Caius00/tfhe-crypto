use axum::http::StatusCode;
use axum::{
    extract::{Path, State},
    Json,
};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
};
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheBool, FheUint8};
use uuid::Uuid;
use crate::voting::types::{
    AppState, ApproveRequest, CreateSessionRequest, CreateSessionResponse,
    JoinRequest, JoinResponse, QuestionType, ResultResponse, SessionState,
    VoteRequest, VoteResponse, ParticipantState, Question,
};
use redis::AsyncCommands;


type ApiError = (StatusCode, String);
type ApiResult<T> = Result<Json<T>, ApiError>;

fn err(msg: &str) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, msg.to_string())
}

/// POST /session – Ersteller legt neue Voting-Session an
/// erstellen einer neuen SessionState mit leeren participants, votes, usw.
/// generiert zufällig eine UUID als session_id und speichert die Session in HashMap
pub async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> ApiResult<CreateSessionResponse> {
    let sk_bytes = general_purpose::STANDARD
        .decode(&req.server_key)
        .map_err(|e| err(&format!("Invalid ServerKey Base64: {}", e)))?;
    let _: CompressedServerKey = bincode::deserialize(&sk_bytes)
        .map_err(|e| err(&format!("Failed to deserialize ServerKey: {}", e)))?;

    let session_id = Uuid::new_v4().to_string();
    // im Handler
let session = SessionState {
    creator_id: req.creator_id,
    server_key_bytes: sk_bytes,
    public_key: req.public_key.clone(), // neu
    questions: req.questions,
    participants: HashMap::new(),
    votes: HashMap::new(),
    finalized: false,
    encrypted_results: None,
};


    state.lock().unwrap().insert(session_id.clone(), session);
    Ok(Json(CreateSessionResponse { session_id }))
}

/// POST /join – Teilnehmer beantragt Teilnahme
pub async fn join_session(
    State(state): State<AppState>,
    Json(req): Json<JoinRequest>,
) -> ApiResult<JoinResponse> {
    let mut map = state.lock().unwrap();
    let session = map
        .get_mut(&req.session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.finalized {
        return Err(err("Session bereits beendet"));
    }

    // Speichere participant state mit enc_name_chunks
    session.participants.insert(
        req.participant_id.clone(),
        ParticipantState {
            approved: false,
            enc_name_chunks: req.enc_name_chunks.clone(),
        },
    );

    Ok(Json(JoinResponse {
        status: "pending".to_string(),
    }))
}


#[derive(Serialize, Deserialize)]
pub struct PendingEntry {
    pub participant_id: String,
    pub enc_name_chunks: Option<Vec<String>>,
}

pub async fn get_pending(
    State(state): State<AppState>,
    Path((session_id, creator_id)): Path<(String, String)>,
) -> ApiResult<Vec<PendingEntry>> {
    let map = state.lock().unwrap();
    let session = map
        .get(&session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != creator_id {
        return Err(err("Nicht autorisiert"));
    }

    let pending: Vec<PendingEntry> = session
        .participants
        .iter()
        .filter(|(_, p)| !p.approved)
        .map(|(id, p)| PendingEntry {
            participant_id: id.clone(),
            enc_name_chunks: p.enc_name_chunks.clone(),
        })
        .collect();

    Ok(Json(pending))
}


/// POST /approve – Ersteller genehmigt oder lehnt Teilnehmer ab
pub async fn approve_participant(
    State(state): State<AppState>,
    Json(req): Json<ApproveRequest>,
) -> ApiResult<serde_json::Value> {
    let mut map = state.lock().unwrap();
    let session = map
        .get_mut(&req.session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != req.creator_id {
        return Err(err("Nicht autorisiert"));
    }

    if req.approved {
    if let Some(p) = session.participants.get_mut(&req.participant_id) {
        p.approved = true;
    }
} else {
    session.participants.remove(&req.participant_id);
}


    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// POST /vote – Teilnehmer gibt verschlüsselte Stimmen ab
/// verschlüsselte Stimmen werden in session.votes gespeichert
pub async fn submit_vote(
    State(state): State<AppState>,
    Json(req): Json<VoteRequest>,
) -> ApiResult<VoteResponse> {
    let mut map = state.lock().unwrap();
    let session = map
        .get_mut(&req.session_id)
        .ok_or(err("Session nicht gefunden"))?;

    // Hole ParticipantState
    let participant = session
        .participants
        .get(&req.participant_id)
        .ok_or(err("Teilnehmer nicht in Session"))?;

    if !participant.approved {
        return Err(err("Teilnehmer noch nicht genehmigt"));
    }

    if session.finalized {
        return Err(err("Session bereits beendet"));
    }
    if req.encrypted_votes.len() != session.questions.len() {
        return Err(err("Anzahl der Stimmen stimmt nicht mit Fragen überein"));
    }

    session.votes.insert(req.participant_id.clone(), req.encrypted_votes);

    println!("=== VOTE STORED ===");
    println!("participant: {}", req.participant_id);
    println!("votes per question: {:?}", session.votes.len());
    println!("raw votes: {:?}", session.votes);

    Ok(Json(VoteResponse {
        status: "vote received".to_string(),
    }))
}


pub fn aggregate_votes_ciphertext_only(
    votes: &Vec<Vec<String>>,
    questions: &Vec<Question>,
    server_key_bytes: &[u8],
) -> Vec<String> {
    let compressed_key: CompressedServerKey =
        bincode::deserialize(server_key_bytes).expect("Failed to deserialize server key");
    let server_key = compressed_key.decompress();
    tfhe::set_server_key(server_key);

    let mut results = vec![];

    for (q_idx, question) in questions.iter().enumerate() {
        let mut acc_uint: Option<FheUint8> = None;

        for v in votes {
            let bytes = general_purpose::STANDARD
                .decode(&v[q_idx])
                .expect("Failed to base64-decode vote");

            match question.question_type {
                QuestionType::Bool => {
                    // deserialize as FheBool, cast to FheUint8 (0/1) and add
                    let vote_bool: FheBool = bincode::deserialize(&bytes)
                        .expect("Failed to deserialize FheBool");
                    let vote_uint: FheUint8 = vote_bool.cast_into();
                    acc_uint = Some(match acc_uint {
                        None => vote_uint,
                        Some(prev) => prev + vote_uint,
                    });
                }
                QuestionType::Single | QuestionType::Multiple | QuestionType::Numeric => {
                    // numeric/choice stored as FheUint8
                    let vote_uint: FheUint8 = bincode::deserialize(&bytes)
                        .expect("Failed to deserialize FheUint8");
                    acc_uint = Some(match acc_uint {
                        None => vote_uint,
                        Some(prev) => prev + vote_uint,
                    });
                }
            }
        }

        if let Some(sum_uint) = acc_uint {
            let serialized = bincode::serialize(&sum_uint).expect("Failed to serialize sum");
            results.push(general_purpose::STANDARD.encode(serialized));
        } else {
            results.push(String::new());
        }
    }

    results
}



/// GET /results/:session_id/:creator_id – Ersteller pollt das verschlüsselte Ergebnis
pub async fn get_results(
    State(state): State<AppState>,
    Path((session_id, creator_id)): Path<(String, String)>,
) -> ApiResult<ResultResponse> {
    let map = state.lock().unwrap();

    let session = map
        .get(&session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != creator_id {
        return Err(err("Nicht autorisiert"));
    }
    let approved_count = session.participants.values().filter(|p| p.approved).count();
    let voted_count = session.votes.len();

    println!("=== GET RESULTS DEBUG ===");
    println!("approved_count: {}", approved_count);
    println!("voted_count: {}", voted_count);

    if voted_count == 0 || voted_count < approved_count {
        println!("→ ready: false");
        return Ok(Json(ResultResponse {
            encrypted_results: vec![],
            ready: false,
        }));
    }
    let votes: Vec<Vec<String>> =
        session.votes.values().cloned().collect();

    let results = aggregate_votes_ciphertext_only(
    &votes,
    &session.questions,
    &session.server_key_bytes,
);


    Ok(Json(ResultResponse {
        encrypted_results: results,
        ready: true,
    }))
}

pub async fn finalize_session(
    State(state): State<AppState>,
    Path((session_id, creator_id)): Path<(String, String)>,
) -> ApiResult<serde_json::Value> {
    let mut map = state.lock().unwrap();

    let session = map
        .get_mut(&session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != creator_id {
        return Err(err("Nicht autorisiert"));
    }

    session.finalized = true;

    Ok(Json(serde_json::json!({ "status": "finalized" })))
}

pub async fn get_status(
    State(state): State<AppState>,
    Path((session_id, participant_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let map = state.lock().unwrap();

    let session = match map.get(&session_id) {
        Some(s) => s,
        None => {
            return Err((StatusCode::NOT_FOUND, "Session nicht gefunden".to_string()));
        }
    };

    println!(
        "STATUS CHECK -> session: {}, participant: {}",
        session_id, participant_id
    );

    let status = match session.participants.get(&participant_id) {
        Some(p) if p.approved => "approved",
        Some(_) => "pending",
        None => "not_found",
    };

    Ok(Json(serde_json::json!({
        "status": status
    })))
}


//
// GET SESSION
//
pub async fn get_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let map = state.lock().unwrap();

    let session = map
        .get(&session_id)
        .ok_or(err("Session nicht gefunden"))?;

    Ok(Json(serde_json::json!({
        "session_id": session_id,
        "questions": session.questions,
        "public_key": session.public_key, // neu
    })))
}

// Endpunkte für den Datenbankzugriff und die Verwaltung des private Keys
async fn get_redis() -> Result<redis::aio::MultiplexedConnection, ApiError> {
    let client = redis::Client::open("redis://localhost:6379")
        .map_err(|e| err(&format!("Redis Verbindung fehlgeschlagen: {}", e)))?;
    client.get_multiplexed_async_connection().await
        .map_err(|e| err(&format!("Redis Connection fehlgeschlagen: {}", e)))
}

/// POST /store-key – speichert den ClientKey in Redis
pub async fn store_client_key(
    Json(req): Json<serde_json::Value>,
) -> ApiResult<serde_json::Value> {
    let session_id = req["session_id"].as_str().ok_or(err("session_id fehlt"))?;
    let client_key_b64 = req["client_key"].as_str().ok_or(err("client_key fehlt"))?;

    let mut con = get_redis().await?;
    let key = format!("voting:clientkey:{}", session_id);

    // 24 Stunden TTL
    con.set_ex::<_, _, ()>(&key, client_key_b64, 86400).await
        .map_err(|e| err(&format!("Redis SET fehlgeschlagen: {}", e)))?;

    Ok(Json(serde_json::json!({ "status": "ok" })))
}

/// GET /load-key/:session_id – lädt den ClientKey aus Redis
pub async fn load_client_key(
    Path(session_id): Path<String>,
) -> ApiResult<serde_json::Value> {
    let mut con = get_redis().await?;
    let key = format!("voting:clientkey:{}", session_id);

    let value: Option<String> = con.get(&key).await
        .map_err(|e| err(&format!("Redis GET fehlgeschlagen: {}", e)))?;

    match value {
        Some(client_key_b64) => Ok(Json(serde_json::json!({ "client_key": client_key_b64 }))),
        None => Err(err("Kein ClientKey gefunden")),
    }
}
