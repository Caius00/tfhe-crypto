use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
pub struct Question {
    pub id: u32,
    pub text: String,
    pub question_type: QuestionType,
    pub options: Option<Vec<String>>,
    pub multiple: Option<bool>, // optional, falls Frontend dieses Feld nutzt
}

#[derive(Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum QuestionType {
    Bool,
    Single,
    Multiple,
    Numeric,
}

#[derive(Serialize, Deserialize, Clone, JsonSchema)]
pub struct ParticipantState {
    pub approved: bool,
    pub enc_name_chunks: Option<Vec<String>>, // optional, wird beim Join übergeben
    pub has_voted: bool,
}

// SessionState ist nicht Teil der API – kein JsonSchema nötig.
#[derive(Serialize, Deserialize, Clone)]
pub struct SessionState {
    pub creator_id: String,
    pub server_key_bytes: Vec<u8>,
    pub public_key: Option<String>, // Base64 public key
    pub questions: Vec<Question>,
    pub participants: HashMap<String, ParticipantState>,
    pub votes: HashMap<String, Vec<Vec<String>>>,
    pub finalized: bool,
    pub encrypted_results: Option<Vec<String>>,
}

pub type AppState = Arc<Mutex<HashMap<String, SessionState>>>;

// ─── Request / Response DTOs ─────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct CreateSessionRequest {
    pub creator_id: String,
    pub server_key: String,         // Base64 CompressedServerKey
    pub public_key: Option<String>, // optional: Base64 public key (für Teilnehmer)
    pub questions: Vec<Question>,
}

#[derive(Serialize, JsonSchema)]
pub struct CreateSessionResponse {
    pub session_id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct JoinRequest {
    pub session_id: String,
    pub participant_id: String,
    pub enc_name_chunks: Option<Vec<String>>, // optional: array of Base64 chunks
}

#[derive(Serialize, JsonSchema)]
pub struct JoinResponse {
    pub status: String, // "pending"
}

#[derive(Deserialize, JsonSchema)]
pub struct ApproveRequest {
    pub session_id: String,
    pub creator_id: String,
    pub participant_id: String,
    pub approved: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct VoteRequest {
    pub session_id: String,
    pub participant_id: String,
    pub encrypted_votes: Vec<Vec<String>>,
}

#[derive(Serialize, JsonSchema)]
pub struct VoteResponse {
    pub status: String,
}

#[derive(Serialize, JsonSchema)]
pub struct ResultResponse {
    pub encrypted_results: Vec<Vec<String>>,
    pub ready: bool,
}

// ─── Generische kleine Response-Structs (ersetzen serde_json::Value) ─────────

/// Allgemeine OK-Antwort mit einem `status`-Feld.
#[derive(Serialize, JsonSchema)]
pub struct StatusResponse {
    pub status: String,
}

/// Antwort auf `GET /status/:session_id/:participant_id`:
/// `approved` | `pending` | `not_found`.
#[derive(Serialize, JsonSchema)]
pub struct ParticipantStatusResponse {
    pub status: String,
}

#[derive(Serialize, Deserialize, JsonSchema, Clone)]
pub struct ParticipantAdminView {
    pub participant_id: String,
    pub approved: bool,
    pub has_voted: bool,
    pub enc_name_chunks: Option<Vec<String>>,
}

/// Antwort auf `GET /session/:session_id` – die für Teilnehmer sichtbaren Felder.
#[derive(Serialize, JsonSchema)]
pub struct SessionInfoResponse {
    pub session_id: String,
    pub questions: Vec<Question>,
    pub public_key: Option<String>,
}
