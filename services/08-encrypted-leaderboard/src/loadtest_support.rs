//! Geteilte Helfer für Integrationstests und Loadtest-Tooling.
//!
//! Sowohl die Integrationstests (`tests/api.rs`) als auch der Corpus-Generator
//! (`src/bin/gen_corpus.rs`) brauchen exakt dieselbe TFHE-Schlüssel-Erzeugung
//! und Ciphertext-Kodierung. Statt den Code zu duplizieren, lebt er hier.
//!
//! Die Schlüsselerzeugung dauert ~30–60 s. Über `OnceLock` passiert sie
//! genau einmal pro Prozess — auch wenn `keys()` mehrfach aufgerufen wird,
//! wird das Material nur einmal generiert.
//!
//! Das Modul ist `pub`, damit Tests und Binaries es importieren können.
//! Der laufende `encrypted-leaderboard`-Service ruft `keys()` nie auf und
//! zahlt damit keinen Init-Overhead — der `OnceLock` bleibt unbefüllt.

use std::sync::OnceLock;

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use tfhe::prelude::*;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheBool, FheUint16, FheUint8};

/// Material für einen vollständigen FHE-Roundtrip.
///
/// - `client_key`: Für lokale Verschlüsselung von Klartext-Werten und für die
///   Entschlüsselung der Server-Antworten in Tests.
/// - `server_key_b64`: Base64-kodierter `CompressedServerKey`, wie ihn der
///   Service in `POST /create` erwartet (Service dekomprimiert ihn dann).
pub struct TestKeys {
    pub client_key: ClientKey,
    pub server_key_b64: String,
}

/// Liefert das prozesseigene TFHE-Schlüsselpaar (lazy initialisiert).
///
/// Beim ersten Aufruf werden `ClientKey` und `CompressedServerKey` erzeugt
/// (mehrere zehn Sekunden) und der serialisierte ServerKey base64-kodiert.
/// Folgeaufrufe geben dieselben Referenzen zurück, ohne weiteren Rechenaufwand.
pub fn keys() -> &'static TestKeys {
    static KEYS: OnceLock<TestKeys> = OnceLock::new();
    KEYS.get_or_init(|| {
        let config = ConfigBuilder::default().build();
        let client_key = ClientKey::generate(config);
        let compressed = CompressedServerKey::new(&client_key);
        let server_bytes = bincode::serialize(&compressed).expect("serialize server key");
        TestKeys {
            client_key,
            server_key_b64: B64.encode(server_bytes),
        }
    })
}

/// Verschlüsselt einen u16-Score und gibt ihn als `Base64(bincode(FheUint16))` zurück.
///
/// Format passt zum Body-Schema von `POST /{code}/submit` (`encrypted_score`).
pub fn enc_score(value: u16) -> String {
    let ck = &keys().client_key;
    let enc = FheUint16::try_encrypt(value, ck).expect("encrypt score");
    B64.encode(bincode::serialize(&enc).expect("serialize"))
}

/// Verschlüsselt eine u8-Spieler-ID — Format wie `enc_score`, aber `FheUint8`.
pub fn enc_id(value: u8) -> String {
    let ck = &keys().client_key;
    let enc = FheUint8::try_encrypt(value, ck).expect("encrypt id");
    B64.encode(bincode::serialize(&enc).expect("serialize"))
}

/// Entschlüsselt einen Base64-kodierten `FheUint16` zurück nach Klartext.
pub fn dec_score(b64: &str) -> u16 {
    let bytes = B64.decode(b64).expect("decode");
    let enc: FheUint16 = bincode::deserialize(&bytes).expect("deser");
    enc.decrypt(&keys().client_key)
}

/// Entschlüsselt einen Base64-kodierten `FheUint8` zurück nach Klartext.
pub fn dec_id(b64: &str) -> u8 {
    let bytes = B64.decode(b64).expect("decode");
    let enc: FheUint8 = bincode::deserialize(&bytes).expect("deser");
    enc.decrypt(&keys().client_key)
}

/// Entschlüsselt einen Base64-kodierten `FheBool` (z.B. aus `/rank`-Antworten).
pub fn dec_bool(b64: &str) -> bool {
    let bytes = B64.decode(b64).expect("decode");
    let enc: FheBool = bincode::deserialize(&bytes).expect("deser");
    enc.decrypt(&keys().client_key)
}
