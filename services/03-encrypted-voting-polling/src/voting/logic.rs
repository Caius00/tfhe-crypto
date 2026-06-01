use crate::voting::types::{
    AppState, ApproveRequest, CreateSessionRequest, CreateSessionResponse, JoinRequest,
    JoinResponse, ParticipantAdminView, ParticipantState, ParticipantStatusResponse, Question,
    QuestionType, ResultResponse, SessionInfoResponse, SessionState, StatusResponse, VoteRequest,
    VoteResponse,
};
use axum::http::StatusCode;
use axum::{
    extract::{Path, State},
    Json,
};
use base64::{engine::general_purpose, Engine as _};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;
use tfhe::{CompressedServerKey, FheUint32};
use uuid::Uuid;

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
            has_voted: false,
        },
    );

    Ok(Json(JoinResponse {
        status: "pending".to_string(),
    }))
}

#[derive(Deserialize, JsonSchema)]
pub struct SessionPath {
    /// UUID der Voting-Session.
    pub session_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SessionCreatorPath {
    /// UUID der Voting-Session.
    pub session_id: String,
    /// Kennung des Erstellers (zur Autorisierung).
    pub creator_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SessionParticipantPath {
    /// UUID der Voting-Session.
    pub session_id: String,
    /// Kennung des Teilnehmers.
    pub participant_id: String,
}

/// POST /approve – Ersteller genehmigt oder lehnt Teilnehmer ab
pub async fn approve_participant(
    State(state): State<AppState>,
    Json(req): Json<ApproveRequest>,
) -> ApiResult<StatusResponse> {
    let mut map = state.lock().unwrap();
    let session = map
        .get_mut(&req.session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != req.creator_id {
        return Err(err("Nicht autorisiert"));
    }

    if session.finalized {
        return Err(err("Session bereits beendet"));
    }

    if req.approved {
        if let Some(p) = session.participants.get_mut(&req.participant_id) {
            p.approved = true;
        }
    } else {
        session.participants.remove(&req.participant_id);
    }

    Ok(Json(StatusResponse {
        status: "ok".to_string(),
    }))
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
        .get_mut(&req.participant_id)
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

    participant.has_voted = true;

    session
        .votes
        .insert(req.participant_id.clone(), req.encrypted_votes);

    println!("=== VOTE STORED ===");
    println!("participant: {}", req.participant_id);
    println!("votes per question: {:?}", session.votes.len());
    println!("raw votes: {:?}", session.votes);

    Ok(Json(VoteResponse {
        status: "vote received".to_string(),
    }))
}

pub fn aggregate_votes_ciphertext_only(
    votes: &[Vec<Vec<String>>],
    questions: &[Question],
    server_key_bytes: &[u8],
) -> Vec<Vec<String>> {
    let compressed_key: CompressedServerKey =
        bincode::deserialize(server_key_bytes).expect("Failed to deserialize server key");
    let server_key = compressed_key.decompress();
    tfhe::set_server_key(server_key);

    let mut results: Vec<Vec<String>> = vec![];

    for (q_idx, question) in questions.iter().enumerate() {
        match question.question_type {
            //  BOOL / NUMERIC → einzelne Summe
            QuestionType::Bool | QuestionType::Numeric => {
                let mut acc: Option<FheUint32> = None;

                for v in votes {
                    let bytes = general_purpose::STANDARD.decode(&v[q_idx][0]).unwrap();

                    let vote: FheUint32 = bincode::deserialize(&bytes).unwrap();

                    acc = Some(match acc {
                        None => vote,
                        Some(prev) => prev + vote,
                    });
                }

                let serialized = bincode::serialize(&acc.unwrap()).unwrap();
                results.push(vec![general_purpose::STANDARD.encode(serialized)]);
            }

            // SINGLE / MULTIPLE → Vektor
            QuestionType::Single | QuestionType::Multiple => {
                let option_count = votes[0][q_idx].len();
                let mut acc_vec: Vec<Option<FheUint32>> = vec![None; option_count];

                for v in votes {
                    for (opt_idx, enc) in v[q_idx].iter().enumerate() {
                        let bytes = general_purpose::STANDARD.decode(enc).unwrap();
                        let vote: FheUint32 = bincode::deserialize(&bytes).unwrap();

                        acc_vec[opt_idx] = Some(match &acc_vec[opt_idx] {
                            None => vote,
                            Some(prev) => prev + vote,
                        });
                    }
                }

                let serialized_vec: Vec<String> = acc_vec
                    .into_iter()
                    .map(|v| {
                        let ser = bincode::serialize(&v.unwrap()).unwrap();
                        general_purpose::STANDARD.encode(ser)
                    })
                    .collect();

                results.push(serialized_vec);
            }
        }
    }

    results
}

/// GET /results/:session_id/:creator_id – Ersteller pollt das verschlüsselte Ergebnis
pub async fn get_results(
    State(state): State<AppState>,
    Path(SessionCreatorPath {
        session_id,
        creator_id,
    }): Path<SessionCreatorPath>,
) -> ApiResult<ResultResponse> {
    let map = state.lock().unwrap();

    let session = map.get(&session_id).ok_or(err("Session nicht gefunden"))?;

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
    let votes: Vec<Vec<Vec<String>>> = session.votes.values().cloned().collect();

    let results =
        aggregate_votes_ciphertext_only(&votes, &session.questions, &session.server_key_bytes);

    Ok(Json(ResultResponse {
        encrypted_results: results,
        ready: true,
    }))
}

pub async fn finalize_session(
    State(state): State<AppState>,
    Path(SessionCreatorPath {
        session_id,
        creator_id,
    }): Path<SessionCreatorPath>,
) -> ApiResult<StatusResponse> {
    let mut map = state.lock().unwrap();

    let session = map
        .get_mut(&session_id)
        .ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != creator_id {
        return Err(err("Nicht autorisiert"));
    }

    session.finalized = true;

    Ok(Json(StatusResponse {
        status: "finalized".to_string(),
    }))
}

pub async fn get_status(
    State(state): State<AppState>,
    Path(SessionParticipantPath {
        session_id,
        participant_id,
    }): Path<SessionParticipantPath>,
) -> ApiResult<ParticipantStatusResponse> {
    let map = state.lock().unwrap();

    let session = map
        .get(&session_id)
        .ok_or((StatusCode::NOT_FOUND, "Session nicht gefunden".to_string()))?;

    println!(
        "STATUS CHECK -> session: {}, participant: {}",
        session_id, participant_id
    );

    let status = match session.participants.get(&participant_id) {
        Some(p) if p.approved => "approved",
        Some(_) => "pending",
        None => "not_found",
    };

    Ok(Json(ParticipantStatusResponse {
        status: status.to_string(),
    }))
}

//
// GET SESSION
//
pub async fn get_session(
    State(state): State<AppState>,
    Path(SessionPath { session_id }): Path<SessionPath>,
) -> ApiResult<SessionInfoResponse> {
    let map = state.lock().unwrap();

    let session = map.get(&session_id).ok_or(err("Session nicht gefunden"))?;

    Ok(Json(SessionInfoResponse {
        session_id,
        questions: session.questions.clone(),
        public_key: session.public_key.clone(),
    }))
}

pub async fn get_participants(
    State(state): State<AppState>,
    Path(SessionCreatorPath {
        session_id,
        creator_id,
    }): Path<SessionCreatorPath>,
) -> ApiResult<Vec<ParticipantAdminView>> {
    let map = state.lock().unwrap();
    let session = map.get(&session_id).ok_or(err("Session nicht gefunden"))?;

    if session.creator_id != creator_id {
        return Err(err("Nicht autorisiert"));
    }

    let result: Vec<ParticipantAdminView> = session
        .participants
        .iter()
        .map(|(id, p)| ParticipantAdminView {
            participant_id: id.clone(),
            approved: p.approved,
            has_voted: p.has_voted,
            enc_name_chunks: p.enc_name_chunks.clone(),
        })
        .collect();

    Ok(Json(result))
}
