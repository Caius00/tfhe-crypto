use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use base64::{engine::general_purpose, Engine as _};
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheBool, FheUint8};
use tower::ServiceExt;

#[tokio::test(flavor = "multi_thread")]
async fn test_verify_age_true() {
    // 1. Keys und Verschlüsselung
    let config = ConfigBuilder::default().build();
    let client_key = ClientKey::generate(config);
    let server_key = CompressedServerKey::new(&client_key);

    let age: i8 = 20;
    let encrypted_age = FheInt8::encrypt(age, &client_key);

    // 2. Daten für den Request vorbereiten
    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    let age_payload = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    let payload = AgeRequest {
        encrypted_age: age_payload,
        server_key: sk_payload,
    };

    // 3. Router-Setup
    let app = Router::new()
        .route("/", post(verify_age))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

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
    let config = ConfigBuilder::default().build();
    let client_key = ClientKey::generate(config);
    let server_key = CompressedServerKey::new(&client_key);

    let age: i8 = 17;
    let encrypted_age = FheInt8::encrypt(age, &client_key);

    // 2. Daten für den Request vorbereiten
    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    let age_payload = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    let payload = AgeRequest {
        encrypted_age: age_payload,
        server_key: sk_payload,
    };

    // 3. Router-Setup
    let app = Router::new()
        .route("/", post(verify_age))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

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
    let config = ConfigBuilder::default().build();
    let client_key = ClientKey::generate(config);
    let server_key = CompressedServerKey::new(&client_key);

    let age: i8 = -17;
    let encrypted_age = FheInt8::encrypt(age, &client_key);

    // 2. Daten für den Request vorbereiten
    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    let age_payload = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    let payload = AgeRequest {
        encrypted_age: age_payload,
        server_key: sk_payload,
    };

    // 3. Router-Setup
    let app = Router::new()
        .route("/", post(verify_age))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

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
