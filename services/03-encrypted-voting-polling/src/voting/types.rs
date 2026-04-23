
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: u32,
    pub text: String,
    pub question_type: QuestionType, // "bool" oder "choice"
    pub options: Option<Vec<String>>, // bei Multiple Choice
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionType {
    Bool,
    Choice,
}

#[derive(Clone)]
pub struct SessionState {
    pub creator_id: String,
    pub server_key_bytes: Vec<u8>,
    pub questions: Vec<Question>,
    // participant_id → approved?
    pub participants: HashMap<String, bool>,
    // participant_id → Vec<encrypted_vote_per_question (Base64)>
    pub votes: HashMap<String, Vec<String>>,
    pub finalized: bool,
    pub encrypted_results: Option<Vec<String>>, // Base64 pro Frage
}

pub type AppState = Arc<Mutex<HashMap<String, SessionState>>>;

// ─── Request / Response DTOs ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub creator_id: String,
    pub server_key: String, // Base64 CompressedServerKey
    pub questions: Vec<Question>,
}

#[derive(Serialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
}

#[derive(Deserialize)]
pub struct JoinRequest {
    pub session_id: String,
    pub participant_id: String,
}

#[derive(Serialize)]
pub struct JoinResponse {
    pub status: String, // "pending"
}

#[derive(Deserialize)]
pub struct ApproveRequest {
    pub session_id: String,
    pub creator_id: String,
    pub participant_id: String,
    pub approved: bool,
}

#[derive(Deserialize)]
pub struct VoteRequest {
    pub session_id: String,
    pub participant_id: String,
    // Ein Base64-String pro Frage (FheUint8 oder FheBool, je nach Fragetyp)
    pub encrypted_votes: Vec<String>,
}

#[derive(Serialize)]
pub struct VoteResponse {
    pub status: String,
}

#[derive(Serialize)]
pub struct ResultResponse {
    // Base64-kodiertes verschlüsseltes Ergebnis pro Frage
    // (FheUint8 = Summe der Stimmen pro Option ODER FheBool = Mehrheit)
    pub encrypted_results: Vec<String>,
    pub ready: bool,
}
