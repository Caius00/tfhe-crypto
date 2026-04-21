use tfhe::FheUint8;
use serde::Serialize;
use std::collections::HashMap;

/// Die Voting Session — erstellt von Client E
pub struct VotingSession {
    pub id: u32,
    pub question: String,
    pub options: Vec<String>,         // Optionsnamen dürfen Klartext sein
    pub tallies: Vec<FheUint8>,       // verschlüsselte Summe pro Option
    pub approved_voters: Vec<String>, // zugelassene Voter-IDs (Klartext)
    pub pending_requests: Vec<PendingRequest>, // noch nicht genehmigt
    pub votes_cast: Vec<String>,      // wer hat schon abgestimmt
}

pub struct PendingRequest {
    pub voter_id: String,
    pub encrypted_name: FheUint8,
}
/// Eine abgegebene Stimme
pub struct EncryptedBallot {
    pub session_id: u32,              // Klartext
    pub voter_id: String,             // Klartext
    pub encrypted_name: FheUint8,     // verschlüsselter Name
    pub choices: Vec<FheUint8>,       // eine FheUint8 pro Option (0 oder 1)
}

/// Struct für HTTP-Antwort
#[derive(Serialize)]
pub struct SessionInfo {
    pub id: u32,
    pub question: String,
    pub options: Vec<String>,
    pub vote_count: usize,
    pub approved_voters: Vec<String>,
}

/// Struct für Request Body, um Voter zuzulassen
#[derive(serde::Deserialize)]
pub struct ApproveRequest {
    pub voter_id: String,
}

/// Struct für Request, für verschlüsselte Antwort eines zugelassenen Voters
#[derive(serde::Deserialize)]
pub struct VoteRequest{
    pub session_id: u32,
    pub voter_id: String,
    pub encrypted_name: String, // base64 kodiertes FheUint8
    pub choices: Vec<String>, // base64 kodiertes FheUint8 pro Option
}