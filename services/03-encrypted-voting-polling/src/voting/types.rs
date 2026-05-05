use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Serialize, Deserialize)]
pub struct Question {
    pub id: u32,
    pub text: String,
    pub question_type: QuestionType,
    pub options: Option<Vec<String>>,
    pub multiple: Option<bool>, // optional, falls Frontend dieses Feld nutzt
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QuestionType {
    Bool,
    Single,
    Multiple,
    Numeric,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ParticipantState {
    pub approved: bool,
    pub enc_name_chunks: Option<Vec<String>>, // optional, wird beim Join übergeben
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SessionState {
    pub creator_id: String,
    pub server_key_bytes: Vec<u8>,
    pub public_key: Option<String>, // Base64 public key
    pub questions: Vec<Question>,
    pub participants: HashMap<String, ParticipantState>,
    pub votes: HashMap<String, Vec<String>>,
    pub finalized: bool,
    pub encrypted_results: Option<Vec<String>>,
}

pub type AppState = Arc<Mutex<HashMap<String, SessionState>>>;

// ─── Request / Response DTOs ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub creator_id: String,
    pub server_key: String, // Base64 CompressedServerKey
    pub public_key: Option<String>, // optional: Base64 public key (für Teilnehmer)
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
    pub enc_name_chunks: Option<Vec<String>>, // optional: array of Base64 chunks
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
    pub encrypted_results: Vec<String>,
    pub ready: bool,
}

