use tfhe::{FheUint8, ClientKey, ServerKey};
use tfhe::prelude::*;
use super::types::{VotingSession, VoteRequest, EncryptedBallot, PendingRequest};

/// CLIENT E: Neue Session erstellen
pub fn create_session(
    id: u32,
    question: &str,
    options: Vec<&str>,
) -> VotingSession {
    // Tallies mit verschlüsselter 0 initialisieren
    // set_server_key muss vorher gesetzt sein
    let tallies = (0..options.len())
        .map(|_| FheUint8::encrypt_trivial(0u8))
        .collect();

    VotingSession {
        id,
        question: question.to_string(),
        options: options.iter().map(|s| s.to_string()).collect(),
        tallies,
        approved_voters: Vec::new(),
        pending_requests: Vec::new(),
        votes_cast: Vec::new(),
    }
}

/// VOTER: Zulassung anfragen
/// Der verschlüsselte Name wird als einzelnes Byte kodiert (vereinfacht)
pub fn request_approval(
    voter_id: &str,
    name_as_byte: u8,
    client_key: &ClientKey, // Voter nutzt E's PublicKey in echtem System
    // hier vereinfacht mit ClientKey
) -> PendingRequest {
    PendingRequest {
        voter_id: voter_id.to_string(),
        encrypted_name: FheUint8::encrypt(name_as_byte, client_key),
    }
}

/// CLIENT E: Voter genehmigen — verschiebt aus pending in approved
pub fn approve_voter(session: &mut VotingSession, voter_id: &str) {
    // Anfrage aus pending entfernen
    session.pending_requests.retain(|r| r.voter_id != voter_id);
    // Voter zulassen
    if !session.approved_voters.contains(&voter_id.to_string()) {
        session.approved_voters.push(voter_id.to_string());
    }
}

/// CLIENT E: Voter ablehnen
pub fn reject_voter(session: &mut VotingSession, voter_id: &str) {
    session.pending_requests.retain(|r| r.voter_id != voter_id);
}

/// SERVER: Prüfen ob Voter zugelassen ist
pub fn is_approved(session: &VotingSession, voter_id: &str) -> bool {
    session.approved_voters.contains(&voter_id.to_string())
}

/// SERVER: Prüfen ob Voter schon abgestimmt hat
pub fn has_voted(session: &VotingSession, voter_id: &str) -> bool {
    session.votes_cast.contains(&voter_id.to_string())
}

/// VOTER: Ballot erstellen
pub fn create_ballot(
    session_id: u32,
    voter_id: &str,
    name_as_byte: u8,
    chosen_index: usize,
    num_options: usize,
    client_key: &ClientKey,
) -> EncryptedBallot {
    let choices = (0..num_options).map(|i| {
        let value: u8 = if i == chosen_index { 1 } else { 0 };
        FheUint8::encrypt(value, client_key)
    }).collect();

    EncryptedBallot {
        session_id,
        voter_id: voter_id.to_string(),
        encrypted_name: FheUint8::encrypt(name_as_byte, client_key),
        choices,
    }
}

/// SERVER: Stimme aggregieren
pub fn cast_vote(
    session: &mut VotingSession,
    ballot: EncryptedBallot,
) -> Result<(), &'static str> {
    if !is_approved(session, &ballot.voter_id) {
        return Err("Voter nicht zugelassen");
    }
    if has_voted(session, &ballot.voter_id) {
        return Err("Voter hat bereits abgestimmt");
    }
    if ballot.choices.len() != session.tallies.len() {
        return Err("Ungültige Anzahl an Optionen");
    }

    // Verschlüsselt addieren
    for (tally, choice) in session.tallies.iter_mut().zip(ballot.choices.iter()) {
        *tally = &*tally + choice;
    }

    session.votes_cast.push(ballot.voter_id);
    Ok(())
}

/// CLIENT E: Ergebnisse entschlüsseln
pub fn decrypt_results(
    session: &VotingSession,
    client_key: &ClientKey,
) -> Vec<(String, u8)> {
    session.options.iter().zip(session.tallies.iter()).map(|(name, tally)| {
        let count: u8 = tally.decrypt(client_key);
        (name.clone(), count)
    }).collect()
}