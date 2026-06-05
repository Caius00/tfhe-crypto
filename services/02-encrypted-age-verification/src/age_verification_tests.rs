use super::*;
use std::fs;


use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose, Engine as _};
use serial_test::serial;
use std::sync::OnceLock;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheBool};
use tower::ServiceExt;

// Im Test Setup werden einmalig Server und Client-Key für alle Tests generiert.
static TEST_SETUP: OnceLock<(ClientKey, CompressedServerKey)> = OnceLock::new();

fn get_test_setup() -> &'static (ClientKey, CompressedServerKey) {
    TEST_SETUP.get_or_init(|| {
        let config = ConfigBuilder::default().build();
        let ck = ClientKey::generate(config);
        let sk = CompressedServerKey::new(&ck);
        (ck, sk)
    })
}

/// Ein Client könnte versehentlich oder absichtlich keinen validen base64-String
/// schicken. Stellt sicher, dass der Fehler sauber als BAD_REQUEST zurückkommt
#[test]
fn test_decode_server_key_invalid_base64() {
    let result = decode_server_key("not-valid-base64!!!");
    assert!(result.is_err());
    assert_eq!(result.err().unwrap().0, StatusCode::BAD_REQUEST);
}

/// Gültiges base64, aber der Inhalt ist kein serialisierter CompressedServerKey.
/// Stellt sicher, dass der bincode-Deserialisierungsfehler korrekt als
/// BAD_REQUEST behandelt wird und nicht als 500.
#[test]
fn test_decode_server_key_corrupt_bytes() {
    let corrupt = general_purpose::STANDARD.encode(vec![1, 2, 3, 4, 5]);
    let result = decode_server_key(&corrupt);
    assert!(result.is_err());
    assert_eq!(result.err().unwrap().0, StatusCode::BAD_REQUEST);
}

/// Gleiche Logik wie beim ServerKey: ungültiges base64 im encrypted_age-Feld
/// muss als BAD_REQUEST zurückkommen. Die Fehlerbehandlung beider Felder ist
/// unabhängig und muss separat getestet werden.
#[test]
fn test_decode_encrypted_age_invalid_base64() {
    let result = decode_encrypted_age("not-valid-base64!!!");
    assert!(result.is_err());
    assert_eq!(result.err().unwrap().0, StatusCode::BAD_REQUEST);
}

/// Gültiges base64, aber kein serialisierter FheInt8. Stellt sicher, dass ein
/// Client der z.B. den falschen Wert base64-kodiert hat einen verständlichen
/// Fehler bekommt und der Server stabil bleibt.
#[test]
fn test_decode_encrypted_age_corrupt_bytes() {
    let corrupt = general_purpose::STANDARD.encode(vec![1, 2, 3, 4, 5]);
    let result = decode_encrypted_age(&corrupt);
    assert!(result.is_err());
    assert_eq!(result.err().unwrap().0, StatusCode::BAD_REQUEST);
}

// FHE Unit-Tests (FHE-Logik direkt, kein HTTP)

/// Alle Grenzwerte der age_check()-Funktion in einem FHE-Kontext.
/// Ein einziger Test statt mehrerer, um Key-Generierung nur einmal zu zahlen.
///
/// Grenzwerte:
///   17  → false  (unter 18)
///   18  → true   (exakter Grenzwert von gt(17))
///   20  → true   (normaler positiver Fall)
///   0   → false  (Nullwert: ge(0) ist true, aber gt(17) ist false)
///   -1  → false  (negativer Grenzwert: ge(0) schlägt fehl)
///   -17 → false  (negativer Wert)
///   127 → true   (i8::MAX)
#[test]
#[serial]
fn test_age_check_boundary_values() {
    let (client_key, server_key) = get_test_setup();
    tfhe::set_server_key(server_key.decompress());

    let cases: &[(i8, bool, &str)] = &[
        (17, false, "17  → nicht volljährig"),
        (18, true, "18  → volljährig (exakter Grenzwert)"),
        (20, true, "20  → volljährig"),
        (0, false, "0   → nicht volljährig"),
        (-1, false, "-1  → nicht volljährig (negativer Grenzwert)"),
        (-17, false, "-17 → nicht volljährig"),
        (127, true, "127 → volljährig (i8::MAX)"),
    ];

    for &(age, expected, label) in cases {
        let enc_age = FheInt8::encrypt(age, client_key);
        let result: bool = age_check(&enc_age).decrypt(client_key);
        assert_eq!(result, expected, "Falsches Ergebnis für: {}", label);
    }
}

// Der einzige Test der den kompletten Pfad von Ende zu Ende prüft:
// Client-seitige Verschlüsselung → base64/bincode Serialisierung → HTTP POST
// → FHE-Berechnung auf dem Server → Deserialisierung → Client-seitige
// Entschlüsselung. Fängt Integrationsfehler die in den Unit-Tests unsichtbar
// wären, z.B. wenn encode_result() und decode_encrypted_age() inkompatible
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_verify_age_full_roundtrip() {
    let (client_key, server_key) = get_test_setup();

    let age: i8 = 20;
    let encrypted_age = FheInt8::encrypt(age, client_key);

    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(server_key).unwrap());
    let age_payload = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    fs::write("payload_age.txt", &age_payload).unwrap();
    fs::write("payload_sk.txt", &sk_payload).unwrap();

    let payload = AgeRequest {
        encrypted_age: age_payload,
        server_key: sk_payload,
    };

    let app = create_app();
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();

    let age_res: AgeResponse = serde_json::from_slice(&body_bytes).unwrap();
    let res_bytes = general_purpose::STANDARD.decode(&age_res.is_adult).unwrap();
    let enc_result: FheBool = bincode::deserialize(&res_bytes).unwrap();

    assert!(
        enc_result.decrypt(client_key),
        "User (20) sollte als volljährig erkannt werden"
    );
}
