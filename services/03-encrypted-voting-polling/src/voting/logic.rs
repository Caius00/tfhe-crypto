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
    VoteRequest, VoteResponse,
};

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
    let session = SessionState {
        creator_id: req.creator_id,
        server_key_bytes: sk_bytes,
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

    session
        .participants
        .entry(req.participant_id)
        .or_insert(false);

    Ok(Json(JoinResponse {
        status: "pending".to_string(),
    }))
}

/// GET /pending/:session_id/:creator_id – Ersteller ruft ausstehende Anfragen ab
/// nur ersteller darf das aufrufen
pub async fn get_pending(
    State(state): State<AppState>,
    Path((session_id, creator_id)): Path<(String, String)>,
) -> ApiResult<Vec<String>> {
    let map = state.lock().unwrap();
    let session = map
        .get(&session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != creator_id {
        return Err(err("Nicht autorisiert"));
    }

    let pending: Vec<String> = session
        .participants
        .iter()
        .filter(|(_, &approved)| !approved)
        .map(|(id, _)| id.clone())
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
        session.participants.insert(req.participant_id.clone(), true);
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

    let approved = session
        .participants
        .get(&req.participant_id)
        .copied()
        .ok_or(err("Teilnehmer nicht in Session"))?;

    if !approved {
        return Err(err("Teilnehmer noch nicht genehmigt"));
    }
    if session.finalized {
        return Err(err("Session bereits beendet"));
    }
    if req.encrypted_votes.len() != session.questions.len() {
        return Err(err("Anzahl der Stimmen stimmt nicht mit Fragen überein"));
    }

    session.votes.insert(req.participant_id.clone(), req.encrypted_votes);

    Ok(Json(VoteResponse {
        status: "vote received".to_string(),
    }))
}

/// GET /results/:session_id/:creator_id – Ersteller pollt das verschlüsselte Ergebnis
pub async fn get_results(
    State(state): State<AppState>,
    Path((session_id, creator_id)): Path<(String, String)>,
) -> ApiResult<ResultResponse> {
    let mut map = state.lock().unwrap();
    let session = map
        .get_mut(&session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != creator_id {
        return Err(err("Nicht autorisiert"));
    }

    let approved_count = session.participants.values().filter(|&&a| a).count();
    let voted_count = session.votes.len();

    if voted_count == 0 || voted_count < approved_count {
        return Ok(Json(ResultResponse {
            encrypted_results: vec![],
            ready: false,
        }));
    }

    if let Some(ref results) = session.encrypted_results {
        return Ok(Json(ResultResponse {
            encrypted_results: results.clone(),
            ready: true,
        }));
    }

    let compressed: CompressedServerKey = bincode::deserialize(&session.server_key_bytes)
        .map_err(|e| err(&format!("ServerKey Fehler: {}", e)))?;

    let questions = session.questions.clone();
    let votes_snapshot: Vec<Vec<String>> = session.votes.values().cloned().collect();

    let encrypted_results: Vec<String> = tokio::task::block_in_place(|| {
        let server_key = compressed.decompress();
        tfhe::set_server_key(server_key);

        let mut results = Vec::new();

        for (q_idx, question) in questions.iter().enumerate() {
            match question.question_type {
                QuestionType::Bool => {
                    let one = FheUint8::encrypt_trivial(1u8);
                    let zero = FheUint8::encrypt_trivial(0u8);
                    let mut total = FheUint8::encrypt_trivial(0u8);

                    for voter_votes in votes_snapshot.iter() {
                        let bytes = general_purpose::STANDARD
                            .decode(&voter_votes[q_idx])
                            .expect("Base64");
                        let vote: FheBool = bincode::deserialize(&bytes).expect("deserialize");
                        total = total + vote.select(&one, &zero);
                    }

                    let res_bytes = bincode::serialize(&total).expect("serialize");
                    results.push(general_purpose::STANDARD.encode(res_bytes));
                }

                QuestionType::Choice => {
                    let option_count = question.options.as_ref().map(|o| o.len()).unwrap_or(2);
                    let one = FheUint8::encrypt_trivial(1u8);
                    let zero = FheUint8::encrypt_trivial(0u8);

                    let mut option_sums: Vec<FheUint8> = (0..option_count)
                        .map(|_| FheUint8::encrypt_trivial(0u8))
                        .collect();

                    for voter_votes in votes_snapshot.iter() {
                        let bytes = general_purpose::STANDARD
                            .decode(&voter_votes[q_idx])
                            .expect("Base64");
                        let vote: FheUint8 = bincode::deserialize(&bytes).expect("deserialize");

                        for (opt_idx, sum) in option_sums.iter_mut().enumerate() {
                            let is_this_option: FheBool = vote.eq(opt_idx as u8);
                            *sum = sum.clone() + is_this_option.select(&one, &zero);
                        }
                    }

                    let serialized: Vec<String> = option_sums
                        .iter()
                        .map(|s| {
                            general_purpose::STANDARD
                                .encode(bincode::serialize(s).expect("serialize"))
                        })
                        .collect();
                    results.push(serde_json::to_string(&serialized).expect("json"));
                }
            }
        }
        results
    });

    session.encrypted_results = Some(encrypted_results.clone());
    session.finalized = true;

    Ok(Json(ResultResponse {
        encrypted_results,
        ready: true,
    }))
}
