//! Base64-Codec für den HTTP-Transport von FHE-Ciphertexts.
//!
//! Hintergrund: FHE-Ciphertexts (bincode-serialisiert) sind beliebige Bytes,
//! die in einer JSON-Payload nicht 1:1 transportiert werden können (JSON kennt
//! kein `Vec<u8>` mit Steuerzeichen). Daher wrappen wir sie in Standard-Base64.
//!
//! Standard-Alphabet (mit `+/` und `=`-Padding) — bewusst nicht URL-safe, weil
//! die Werte nur im Request-Body landen, nie in URLs.

use base64::{engine::general_purpose, Engine as _};

/// Dekodiert Base64 (Standard-Alphabet, mit Padding) zurück zu Bytes.
/// Fehler werden als String zurückgegeben, damit Handler sie direkt mit
/// `400 Bad Request` weiterreichen können.
pub fn b64_decode(s: &str) -> Result<Vec<u8>, String> {
    general_purpose::STANDARD
        .decode(s)
        .map_err(|e| format!("Invalid base64: {e}"))
}

/// Kodiert Bytes als Base64 (Standard-Alphabet, mit Padding) für HTTP-Antworten.
pub fn b64_encode(b: &[u8]) -> String {
    general_purpose::STANDARD.encode(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_bytes_and_invalid_input_is_rejected() {
        // Round-Trip mit allen 256 Byte-Werten + ein paar Wiederholungen am Ende —
        // stellt sicher dass weder NULL (0x00) noch 0xFF irgendwo verschluckt werden.
        let original: Vec<u8> = (0u8..=255).chain([0, 0, 1, 2, 3]).collect();
        let encoded = b64_encode(&original);
        assert_eq!(b64_decode(&encoded).unwrap(), original);

        // Ungültiges Base64 muss eine klare Fehlermeldung liefern (Handler verwandelt
        // sie in 400 Bad Request).
        let err = b64_decode("not*base64!").expect_err("must reject");
        assert!(err.starts_with("Invalid base64"), "unexpected: {err}");
    }
}
