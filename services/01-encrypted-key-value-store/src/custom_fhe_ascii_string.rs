//! Verschlüsselte ASCII-Strings für den Encrypted Key-Value Store.
//!
//! Wir repräsentieren einen String als `Vec<FheUint8>` — pro Zeichen ein
//! homomorpher 8-Bit-Ciphertext. Damit lassen sich Strings zeichenweise
//! homomorph vergleichen (`eq`) und konditional ersetzen (`if_then_else`),
//! ohne dass der Server jemals den Klartext sieht.
//!
//! Wire-Format: ein String wird beim Transport als `Vec<Vec<u8>>` serialisiert,
//! wobei jedes innere `Vec<u8>` genau ein `bincode::serialize::<FheUint8>(...)`
//! ist. Das passt zum Format, das die TFHE-WASM-Bindings im Browser produzieren
//! (es existiert kein WASM-Pendant zu `CompressedCiphertextList`), und ist
//! konsistent mit den anderen Services (02, 08) im Repository.

use crate::models::AppError;
use std::ops::{BitAnd, Not};
use tfhe::prelude::{FheDecrypt, FheEncrypt, FheEq, FheTrivialEncrypt, IfThenElse};
use tfhe::{ClientKey, FheBool, FheUint8};

/// Klartext-näher Typ: hält die homomorph operierbaren Ciphertexts im Speicher.
/// Wird ausschließlich serverseitig nach dem Dekomprimieren verwendet.
#[derive(Clone)]
pub struct CustomFheAsciiString {
    pub chars: Vec<FheUint8>,
}

/// Transport-/Storage-Form: ein Vektor mit pro-Zeichen bincode-Bytes eines
/// `FheUint8`. Über die Leitung (HTTP, Redis) wandert ausschließlich dieser Typ.
#[derive(Clone)]
pub struct CompressedCustomFheAsciiString {
    pub chunks: Vec<Vec<u8>>,
}

impl CompressedCustomFheAsciiString {
    /// Erzeugt einen komprimierten String aus bereits gepackten Chunks.
    /// Wird typischerweise nach Base64-Decoding der HTTP-Payload aufgerufen.
    pub fn from_chunks(chunks: Vec<Vec<u8>>) -> Self {
        Self { chunks }
    }

    /// Deserialisiert jeden Chunk zu einem `FheUint8` und gibt den
    /// operationsfähigen `CustomFheAsciiString` zurück.
    ///
    /// Fehler: jeder Chunk kann beim bincode-Decoding scheitern (z.B. korrupte
    /// Payload, falsches Format) — dann gibt es `AppError::BadRequest`.
    pub fn decompress(&self) -> Result<CustomFheAsciiString, AppError> {
        let chars = self
            .chunks
            .iter()
            .map(|c| bincode::deserialize::<FheUint8>(c))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::BadRequest(format!("invalid encrypted char chunk: {e}")))?;

        Ok(CustomFheAsciiString { chars })
    }
}

impl CustomFheAsciiString {
    /// Testhilfe / clientseitige Initialisierung: verschlüsselt einen
    /// Klartext-String Zeichen für Zeichen mit dem ClientKey.
    /// Wird im Server-Binary nicht benutzt (der Server hat keinen ClientKey),
    /// aber von den Integrationstests.
    pub fn new(str: &str, client_key: &ClientKey) -> CustomFheAsciiString {
        let chars = str
            .bytes()
            .map(|byte| FheUint8::encrypt(byte, client_key))
            .collect();
        CustomFheAsciiString { chars }
    }

    /// Serialisiert jeden Ciphertext einzeln per bincode — Ergebnis ist eine
    /// Liste von Byte-Chunks, die JSON-/Base64-tauglich ist.
    ///
    /// Fehler hier sind in der Praxis nicht zu erwarten (`FheUint8` ist immer
    /// serialisierbar), werden aber sauber durchgereicht, statt zu panicen.
    pub fn compress(&self) -> Result<CompressedCustomFheAsciiString, AppError> {
        let chunks = self
            .chars
            .iter()
            .map(bincode::serialize)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::InternalError(format!("bincode serialize FheUint8: {e}")))?;

        Ok(CompressedCustomFheAsciiString { chunks })
    }
}

/// Homomorpher Gleichheits-Vergleich. Liefert einen `FheBool`, der genau dann
/// "true" entschlüsselt, wenn beide Strings zeichenweise identisch sind.
///
/// Bei unterschiedlicher Länge gibt es einen trivial-verschlüsselten `false`.
/// Wichtig: die Längen-Information ist nicht-geheim — der Server sieht ohnehin
/// `chars.len()` als Metadatum, gleichmäßiges Padding wäre eine zusätzliche
/// Härtung und ist im Threat-Model dokumentiert (siehe spec.md).
impl FheEq for CustomFheAsciiString {
    fn eq(&self, other: Self) -> FheBool {
        if self.chars.len() != other.chars.len() {
            return FheBool::encrypt_trivial(false);
        }
        self.chars
            .iter()
            .zip(other.chars.iter())
            .map(|(a, b)| a.eq(b))
            .reduce(|acc, x| acc.bitand(x))
            // `chars` ist nie leer — leere Strings sollten der Eq-Check oben gar nicht
            // erreichen; falls doch, ist trivial-true semantisch korrekt (zwei leere
            // Strings sind gleich).
            .unwrap_or_else(|| FheBool::encrypt_trivial(true))
    }

    fn ne(&self, other: Self) -> FheBool {
        self.eq(other).not()
    }
}

impl FheDecrypt<String> for CustomFheAsciiString {
    fn decrypt(&self, key: &ClientKey) -> String {
        let bytes = self
            .chars
            .iter()
            .map(|c| c.decrypt(key))
            .collect::<Vec<u8>>();
        String::from_utf8(bytes).expect("encrypted string was not valid UTF-8")
    }
}

/// Konditionale Auswahl auf Strings: pro Zeichen `if cond then a else b`.
/// Wird im `get`-Pfad benutzt, um homomorph genau den Wert "auszuwählen",
/// dessen Schlüssel zur Anfrage passt.
impl IfThenElse<CustomFheAsciiString> for FheBool {
    fn if_then_else(
        &self,
        ct_then: &CustomFheAsciiString,
        ct_else: &CustomFheAsciiString,
    ) -> CustomFheAsciiString {
        assert_eq!(
            ct_then.chars.len(),
            ct_else.chars.len(),
            "if_then_else requires equal-length strings"
        );

        let chars = ct_then
            .chars
            .iter()
            .zip(ct_else.chars.iter())
            .map(|(a, b)| self.if_then_else(a, b))
            .collect::<Vec<FheUint8>>();

        CustomFheAsciiString { chars }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
    use tfhe::{set_server_key, CompressedServerKey};

    /// Helper: passende TFHE-Parameter generieren und Keys auf den aktuellen
    /// Thread setzen. Ist ein 1-zu-1-Klon der gleichlautenden Helfer in den
    /// anderen Tests des Services — bewusst dupliziert, weil das Setup
    /// minimal ist und keine geteilte Test-Crate existiert.
    fn setup_keys() -> ClientKey {
        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());
        client_key
    }

    #[test]
    fn eq_returns_true_for_identical_strings_and_false_for_different() {
        let client_key = setup_keys();

        let enc_a = CustomFheAsciiString::new("Hello World!", &client_key);
        let enc_a_2 = CustomFheAsciiString::new("Hello World!", &client_key);
        let enc_b = CustomFheAsciiString::new("Hello Earth!", &client_key);

        assert!(enc_a.eq(enc_a_2).decrypt(&client_key));
        assert!(!enc_a.eq(enc_b).decrypt(&client_key));
    }

    #[test]
    fn compress_then_decompress_roundtrips_to_same_plaintext() {
        let client_key = setup_keys();

        for s in ["Hello World!", "Hello Earth!", "", "x", "你好世界"] {
            let enc = CustomFheAsciiString::new(s, &client_key);
            let roundtripped = enc.compress().unwrap().decompress().unwrap();
            assert_eq!(roundtripped.decrypt(&client_key), s.to_string());
        }
    }
}
