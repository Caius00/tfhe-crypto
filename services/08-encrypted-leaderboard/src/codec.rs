use base64::{engine::general_purpose, Engine as _};

// Base64 -> Bytes (Standard-Alphabet, mit Padding)
pub fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    general_purpose::STANDARD
        .decode(s)
        .map_err(|e| format!("Invalid base64: {e}"))
}

// Bytes -> Base64 (für HTTP-Transport)
pub fn b64_encode(b: &[u8]) -> String {
    general_purpose::STANDARD.encode(b)
}
