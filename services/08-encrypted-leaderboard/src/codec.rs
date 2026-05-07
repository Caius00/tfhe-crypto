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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_bytes_and_invalid_input_is_rejected() {
        // Round-Trip mit Sonderbytes (inkl. NULL und 0xFF) — keine Klartext-Verlierer
        let original: Vec<u8> = (0u8..=255).chain([0, 0, 1, 2, 3]).collect();
        let encoded = b64_encode(&original);
        assert_eq!(b64_decode(&encoded).unwrap(), original);

        // Ungültiges Base64 muss klar als Fehler zurückkommen
        let err = b64_decode("not*base64!").expect_err("must reject");
        assert!(err.starts_with("Invalid base64"), "unexpected: {err}");
    }
}
