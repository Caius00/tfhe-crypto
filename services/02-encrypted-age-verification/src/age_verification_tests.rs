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
/// muss als BAD_REQUEST zurückkommen.
#[test]
fn test_decode_encrypted_age_invalid_base64() {
    let result = decode_encrypted_age("not-valid-base64!!!");
    assert!(result.is_err());
    assert_eq!(result.err().unwrap().0, StatusCode::BAD_REQUEST);
}

/// Gültiges base64, aber kein serialisierter FheInt8.
#[test]
fn test_decode_encrypted_age_corrupt_bytes() {
    let corrupt = general_purpose::STANDARD.encode(vec![1, 2, 3, 4, 5]);
    let result = decode_encrypted_age(&corrupt);
    assert!(result.is_err());
    assert_eq!(result.err().unwrap().0, StatusCode::BAD_REQUEST);
}

/// Alle Grenzwerte der age_check()-Funktion in einem FHE-Kontext.
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

/// End-to-End-Test des zustandslosen Endpunkts (POST /).
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

/// Setup mit ungültigem ServerKey muss 400 zurückgeben.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_setup_session_invalid_server_key() {
    let app = create_app();

    let payload = serde_json::json!({ "server_key": "not-valid-base64!!!" });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/session")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Verify mit unbekannter session_id muss 404 zurückgeben.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_verify_age_session_not_found() {
    let app = create_app();

    let payload = serde_json::json!({ "encrypted_age": "dGVzdA==" });
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/verify/nicht-existierende-session-id")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// End-to-End-Test des session-basierten Flows:
/// POST /session → POST /verify/:id → DELETE /session/:id
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_session_based_roundtrip() {
    let (client_key, server_key) = get_test_setup();

    let age: i8 = 20;
    let encrypted_age = FheInt8::encrypt(age, client_key);

    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(server_key).unwrap());
    let age_payload = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    let app = create_app();

    // 1. Session aufbauen
    let setup_payload = serde_json::json!({ "server_key": sk_payload });
    let setup_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/session")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&setup_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(setup_response.status(), StatusCode::OK);

    let setup_bytes = axum::body::to_bytes(setup_response.into_body(), 1024)
        .await
        .unwrap();
    let setup_body: serde_json::Value = serde_json::from_slice(&setup_bytes).unwrap();
    let session_id = setup_body["session_id"].as_str().unwrap().to_string();

    // 2. Altersverifikation
    let verify_payload = serde_json::json!({ "encrypted_age": age_payload });
    let verify_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/verify/{}", session_id))
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&verify_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(verify_response.status(), StatusCode::OK);

    let verify_bytes = axum::body::to_bytes(verify_response.into_body(), 10 * 1024 * 1024)
        .await
        .unwrap();
    let age_res: AgeResponse = serde_json::from_slice(&verify_bytes).unwrap();
    let res_bytes = general_purpose::STANDARD.decode(&age_res.is_adult).unwrap();
    let enc_result: FheBool = bincode::deserialize(&res_bytes).unwrap();

    assert!(
        enc_result.decrypt(client_key),
        "User (20) sollte als volljährig erkannt werden"
    );

    // 3. Session löschen
    let delete_response = app
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/session/{}", session_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(delete_response.status(), StatusCode::OK);
}
