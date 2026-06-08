//! Base64-Codec für den HTTP-Transport von FHE-Ciphertexts.
//!
//! Identisch zur Konvention der anderen Services (siehe Service 08): wir nutzen
//! das Standard-Alphabet mit Padding (`+/=`), weil die Werte ausschließlich im
//! Request-Body landen und nie in URLs. Ein FHE-Ciphertext ist nach
//! `bincode::serialize` ein beliebiger Bytestrom — JSON kann das nicht direkt
//! transportieren, daher diese dünne Hüllschicht.

use crate::models::AppError;
use base64::{engine::general_purpose, Engine as _};

/// Einzelner Base64-String → Bytes. Fehler werden als `BadRequest` gemeldet,
/// damit der Client die Korrektur am Wire-Format vornehmen kann.
pub fn b64_decode_single(s: &str) -> Result<Vec<u8>, AppError> {
    general_purpose::STANDARD
        .decode(s)
        .map_err(|e| AppError::BadRequest(format!("invalid base64 payload: {e}")))
}

/// Liste von Base64-Strings → Liste von Byte-Chunks. Behält die Reihenfolge,
/// schlägt beim ersten ungültigen Element fehl (kein Teilerfolg).
pub fn b64_decode_chunks(chunks: &[String]) -> Result<Vec<Vec<u8>>, AppError> {
    chunks.iter().map(|c| b64_decode_single(c)).collect()
}

/// Bytes → Base64-String. Wird für jede ausgehende Ciphertext-Antwort genutzt.
pub fn b64_encode_single(bytes: &[u8]) -> String {
    general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_all_byte_values() {
        // Alle 256 Bytes plus ein paar Wiederholungen — fängt ein
        // versehentliches NULL/0xFF-Verschlucken früh.
        let original: Vec<u8> = (0u8..=255).chain([0, 0, 1, 2, 3]).collect();
        let encoded = b64_encode_single(&original);
        assert_eq!(b64_decode_single(&encoded).unwrap(), original);
    }

    #[test]
    fn invalid_base64_is_a_bad_request() {
        let err = b64_decode_single("not*base64!").unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)));
    }

    #[test]
    fn decode_chunks_aggregates_in_order() {
        let inputs: Vec<String> = vec![
            b64_encode_single(&[1, 2, 3]),
            b64_encode_single(&[4]),
            b64_encode_single(&[5, 6]),
        ];
        let decoded = b64_decode_chunks(&inputs).unwrap();
        assert_eq!(decoded, vec![vec![1, 2, 3], vec![4], vec![5, 6]]);
    }
}
