use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose, Engine as _};
use std::sync::OnceLock;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheBool};
use tower::ServiceExt;

static TEST_SETUP: OnceLock<(ClientKey, CompressedServerKey)> = OnceLock::new();

fn get_test_setup() -> &'static (ClientKey, CompressedServerKey) {
    TEST_SETUP.get_or_init(|| {
        let config = ConfigBuilder::default().build();
        let ck = ClientKey::generate(config);
        let sk = CompressedServerKey::new(&ck);
        (ck, sk)
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn test_verify_age_true() {
    // 1. Keys und Verschlüsselung
    let (client_key, server_key) = get_test_setup();

    let age: i8 = 20;
    let encrypted_age = FheInt8::encrypt(age, client_key);

    // 2. Daten für den Request vorbereiten
    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    let age_payload = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    let payload = AgeRequest {
        encrypted_age: age_payload,
        server_key: sk_payload,
    };

    // 3. Router-Setup
    let app = create_app();

    // 4. Request senden
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

    // 5. Response prüfen
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024) // Max 10MB
        .await
        .unwrap();

    let age_res: AgeResponse = serde_json::from_slice(&body_bytes).unwrap();

    // 6. Ergebnis entschlüsseln
    let res_bytes = general_purpose::STANDARD.decode(&age_res.is_adult).unwrap();
    let enc_result: FheBool = bincode::deserialize(&res_bytes).unwrap();
    let is_adult: bool = enc_result.decrypt(&client_key);

    assert!(is_adult, "User (20) sollte als volljährig erkannt werden");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_verify_age_false() {
    // 1. Keys und Verschlüsselung
    let (client_key, server_key) = get_test_setup();

    let age: i8 = 17;
    let encrypted_age = FheInt8::encrypt(age, client_key);

    // 2. Daten für den Request vorbereiten
    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    let age_payload = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    let payload = AgeRequest {
        encrypted_age: age_payload,
        server_key: sk_payload,
    };

    // 3. Router-Setup
    let app = create_app();

    // 4. Request senden
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

    // 5. Response prüfen
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024) // Max 10MB
        .await
        .unwrap();

    let age_res: AgeResponse = serde_json::from_slice(&body_bytes).unwrap();

    // 6. Ergebnis entschlüsseln
    let res_bytes = general_purpose::STANDARD.decode(&age_res.is_adult).unwrap();
    let enc_result: FheBool = bincode::deserialize(&res_bytes).unwrap();
    let is_adult: bool = enc_result.decrypt(&client_key);

    assert!(
        !is_adult,
        "User (17) sollte nicht als volljährig erkannt werden"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_verify_negative_age_false() {
    // 1. Keys und Verschlüsselung
    let (client_key, server_key) = get_test_setup();

    let age: i8 = -17;
    let encrypted_age = FheInt8::encrypt(age, client_key);

    // 2. Daten für den Request vorbereiten
    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    let age_payload = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    let payload = AgeRequest {
        encrypted_age: age_payload,
        server_key: sk_payload,
    };

    // 3. Router-Setup
    let app = create_app();

    // 4. Request senden
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

    // 5. Response prüfen
    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), 10 * 1024 * 1024) // Max 10MB
        .await
        .unwrap();

    let age_res: AgeResponse = serde_json::from_slice(&body_bytes).unwrap();

    // 6. Ergebnis entschlüsseln
    let res_bytes = general_purpose::STANDARD.decode(&age_res.is_adult).unwrap();
    let enc_result: FheBool = bincode::deserialize(&res_bytes).unwrap();
    let is_adult: bool = enc_result.decrypt(&client_key);

    assert!(
        !is_adult,
        "User (-17) sollte nicht als volljährig erkannt werden"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_verify_age_corrupt_data() {
    let app = create_app();

    let corrupt_base64 = general_purpose::STANDARD.encode(vec![1, 2, 3, 4, 5]);

    let payload = AgeRequest {
        encrypted_age: corrupt_base64.clone(),
        server_key: corrupt_base64,
    };

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

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_health_extended() {
    let app = create_app();

    let endpoints = vec!["/healthz", "/readyz", "/version"];

    for uri in endpoints {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK, "Endpoint {} failed", uri);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_verify_age_invalid_base64() {
    let app = create_app();

    let payload = serde_json::json!({
        "encrypted_age": "not-valid-base64",
        "server_key": "invalid-key"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("Content-Type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
