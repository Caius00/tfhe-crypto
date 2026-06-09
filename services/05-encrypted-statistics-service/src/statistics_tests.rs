use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose, Engine as _};
use serial_test::serial;
use std::sync::{Arc, OnceLock};
use tfhe::prelude::*;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheInt8, FheInt16, FheInt32, FheInt64};
use tower::ServiceExt;

use crate::state::{AppState, Session};

// Keys werden einmalig für alle FHE-Tests generiert.
static SHARED_TEST_KEY_PAIR: OnceLock<(ClientKey, CompressedServerKey)> = OnceLock::new();

fn get_shared_test_key_pair() -> &'static (ClientKey, CompressedServerKey) {
    SHARED_TEST_KEY_PAIR.get_or_init(|| {
        let config = ConfigBuilder::default().build();
        let client_key = ClientKey::generate(config);
        let server_key = CompressedServerKey::new(&client_key);
        (client_key, server_key)
    })
}

// ── Hilfsfunktionen ──────────────────────────────────────────────────────────

fn encrypt_i8_to_base64(value: i8, client_key: &ClientKey) -> String {
    general_purpose::STANDARD.encode(bincode::serialize(&FheInt8::encrypt(value, client_key)).unwrap())
}

fn encrypt_i16_to_base64(value: i16, client_key: &ClientKey) -> String {
    general_purpose::STANDARD.encode(bincode::serialize(&FheInt16::encrypt(value, client_key)).unwrap())
}

fn encrypt_i32_to_base64(value: i32, client_key: &ClientKey) -> String {
    general_purpose::STANDARD.encode(bincode::serialize(&FheInt32::encrypt(value, client_key)).unwrap())
}


/// Baut eine App mit vorinstallierter Session.
///
/// Umgeht den HTTP-Roundtrip zu POST /session in Tests — der FheEngine wird
/// direkt in den AppState eingefügt. Die zurückgegebene session_id kann sofort
/// in POST /-Requests verwendet werden.
async fn app_with_session(server_key: &CompressedServerKey) -> (Router, String) {
    let engine = fhe::FheEngine::from_server_key(server_key.decompress()).unwrap();
    let state = AppState::new();
    let session_id = state.insert(Arc::new(Session::new(Arc::new(engine)))).await;
    (create_app(state), session_id)
}

fn post_to(uri: &str, payload: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(payload).unwrap()))
        .unwrap()
}

// ── POST /session Error-Tests ─────────────────────────────────────────────────

/// Ungültiges Base64 im server_key → 400
#[tokio::test]
async fn test_invalid_server_key_base64() {
    let response = create_app(AppState::new())
        .oneshot(post_to("/session", &serde_json::json!({ "server_key": "not-valid-base64!!!" })))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Gültiges Base64, aber kein CompressedServerKey → 400
#[tokio::test]
async fn test_corrupt_server_key_bytes() {
    let payload = serde_json::json!({
        "server_key": general_purpose::STANDARD.encode(vec![1u8, 2, 3, 4, 5]),
    });
    let response = create_app(AppState::new())
        .oneshot(post_to("/session", &payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ── POST / Error-Tests ────────────────────────────────────────────────────────

/// Unbekannte session_id → 404
#[tokio::test]
async fn test_unknown_session_id() {
    let payload = serde_json::json!({
        "session_id": "00000000-0000-0000-0000-000000000000",
        "encrypted_list": [],
        "bit_width": 16,
    });
    let response = create_app(AppState::new())
        .oneshot(post_to("/", &payload))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

/// Ungültiges Base64 in einem Listenelement → 400
// multi_thread weil block_in_place auf single-thread panict (FHE-Compute-Pfad).
// Hier tritt es nicht auf (Fehler vor block_in_place), aber für Konsistenz.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_invalid_list_item_base64() {
    let (_, server_key) = get_shared_test_key_pair();
    let (app, session_id) = app_with_session(server_key).await;
    let payload = serde_json::json!({
        "session_id": session_id,
        "encrypted_list": ["not-valid-base64!!!"],
        "bit_width": 16,
    });
    let response = app.oneshot(post_to("/", &payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Gültiges Base64, aber kein FheInt16 im Listenelement → 400
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_corrupt_list_item_bytes() {
    let (_, server_key) = get_shared_test_key_pair();
    let (app, session_id) = app_with_session(server_key).await;
    let payload = serde_json::json!({
        "session_id": session_id,
        "encrypted_list": [general_purpose::STANDARD.encode(vec![1u8, 2, 3, 4, 5])],
        "bit_width": 16,
    });
    let response = app.oneshot(post_to("/", &payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Leere Liste → 400
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_empty_list() {
    let (_, server_key) = get_shared_test_key_pair();
    let (app, session_id) = app_with_session(server_key).await;
    let payload = serde_json::json!({
        "session_id": session_id,
        "encrypted_list": [],
        "bit_width": 16,
    });
    let response = app.oneshot(post_to("/", &payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Ungültige Bitbreite → 400
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_invalid_bit_width() {
    let (_, server_key) = get_shared_test_key_pair();
    let (app, session_id) = app_with_session(server_key).await;
    let payload = serde_json::json!({
        "session_id": session_id,
        "encrypted_list": [],
        "bit_width": 64,
    });
    let response = app.oneshot(post_to("/", &payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ── FHE Unit-Tests (direkt, kein HTTP) ───────────────────────────────────────

/// Alle Statistikfunktionen auf bekannten Werten mit FheInt16.
/// Eingabe: [10, 50, 30, 20, 40] → sortiert: [10, 20, 30, 40, 50]
///   sum=150, count=5, min=10, max=50, avg=30, median=30
#[test]
#[serial]
fn test_statistics_functions_int16() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let decompressed_server_key = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(decompressed_server_key.clone()));
    tfhe::set_server_key(decompressed_server_key);

    let plaintext_input = [10i16, 50, 30, 20, 40];
    let encrypted_input: Vec<_> = plaintext_input
        .iter()
        .map(|&value| FheInt16::encrypt(value, client_key))
        .collect();

    let encrypted_sum: FheInt32 = statistics::sum(&encrypted_input);
    let encrypted_average: FheInt32 =
        statistics::average_from_sum(encrypted_sum.clone(), encrypted_input.len());

    let sum: i32 = encrypted_sum.decrypt(client_key);
    let min: i16 = statistics::min(&encrypted_input).decrypt(client_key);
    let max: i16 = statistics::max(&encrypted_input).decrypt(client_key);
    let avg: i32 = encrypted_average.decrypt(client_key);
    let med: i16 = statistics::median(&encrypted_input).decrypt(client_key);
    assert_eq!(encrypted_input.len(), 5, "count");
    assert_eq!(sum, 150, "sum");
    assert_eq!(min, 10, "min");
    assert_eq!(max, 50, "max");
    assert_eq!(avg, 30, "average");
    assert_eq!(med, 30, "median");
}

/// n=1 — Sonderpfad in median() und Trivialfall für alle anderen Funktionen.
#[test]
#[serial]
fn test_statistics_single_element() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let decompressed_server_key = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(decompressed_server_key.clone()));
    tfhe::set_server_key(decompressed_server_key);

    let encrypted_single_element = vec![FheInt16::encrypt(42i16, client_key)];
    let encrypted_sum: FheInt32 = statistics::sum(&encrypted_single_element);
    let encrypted_average: FheInt32 =
        statistics::average_from_sum(encrypted_sum.clone(), encrypted_single_element.len());

    let sum: i32 = encrypted_sum.decrypt(client_key);
    let min: i16 = statistics::min(&encrypted_single_element).decrypt(client_key);
    let max: i16 = statistics::max(&encrypted_single_element).decrypt(client_key);
    let avg: i32 = encrypted_average.decrypt(client_key);
    let med: i16 = statistics::median(&encrypted_single_element).decrypt(client_key);
    assert_eq!(sum, 42, "sum");
    assert_eq!(min, 42, "min");
    assert_eq!(max, 42, "max");
    assert_eq!(avg, 42, "average");
    assert_eq!(med, 42, "median");
}

/// Gerades n → Lower Median.
/// [10, 20, 30, 40] → Index (4-1)/2 = 1 → Median = 20
#[test]
#[serial]
fn test_statistics_even_n_lower_median() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let decompressed_server_key = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(decompressed_server_key.clone()));
    tfhe::set_server_key(decompressed_server_key);

    let encrypted_input: Vec<_> = [10i16, 20, 30, 40]
        .iter()
        .map(|&v| FheInt16::encrypt(v, client_key))
        .collect();

    let encrypted_sum: FheInt32 = statistics::sum(&encrypted_input);
    let encrypted_average: FheInt32 =
        statistics::average_from_sum(encrypted_sum, encrypted_input.len());

    let med: i16 = statistics::median(&encrypted_input).decrypt(client_key);
    let avg: i32 = encrypted_average.decrypt(client_key);
    assert_eq!(med, 20, "lower median");
    assert_eq!(avg, 25, "average");
}

/// Negative Eingabewerte — FheInt16 unterstützt Vorzeichen.
/// [-10, -5, 5, 10] → sum=0, min=-10, max=10, avg=0, median=-5
#[test]
#[serial]
fn test_statistics_negative_values() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let decompressed_server_key = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(decompressed_server_key.clone()));
    tfhe::set_server_key(decompressed_server_key);

    let encrypted_input: Vec<_> = [-10i16, -5, 5, 10]
        .iter()
        .map(|&v| FheInt16::encrypt(v, client_key))
        .collect();

    let encrypted_sum: FheInt32 = statistics::sum(&encrypted_input);
    let encrypted_average: FheInt32 =
        statistics::average_from_sum(encrypted_sum.clone(), encrypted_input.len());

    let sum: i32 = encrypted_sum.decrypt(client_key);
    let min: i16 = statistics::min(&encrypted_input).decrypt(client_key);
    let max: i16 = statistics::max(&encrypted_input).decrypt(client_key);
    let avg: i32 = encrypted_average.decrypt(client_key);
    let med: i16 = statistics::median(&encrypted_input).decrypt(client_key);
    assert_eq!(sum, 0, "sum");
    assert_eq!(min, -10, "min");
    assert_eq!(max, 10, "max");
    assert_eq!(avg, 0, "average");
    assert_eq!(med, -5, "lower median");
}

/// Average Truncation toward zero (nicht Floor).
/// [-3, -2] → avg = -5/2 = -2 (toward zero), nicht -3 (floor)
#[test]
#[serial]
fn test_average_truncation_toward_zero() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let decompressed_server_key = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(decompressed_server_key.clone()));
    tfhe::set_server_key(decompressed_server_key);

    let encrypted_input: Vec<_> = [-3i16, -2]
        .iter()
        .map(|&v| FheInt16::encrypt(v, client_key))
        .collect();

    let encrypted_sum: FheInt32 = statistics::sum(&encrypted_input);
    let encrypted_average: FheInt32 =
        statistics::average_from_sum(encrypted_sum, encrypted_input.len());
    let avg: i32 = encrypted_average.decrypt(client_key);
    assert_eq!(avg, -2, "truncation toward zero");
}

// ── HTTP-Roundtrip-Tests ──────────────────────────────────────────────────────

/// Vollständiger HTTP-Roundtrip mit bit_width=16.
/// Eingabe: [10, 30, 20] → sum=60, count=3, min=10, max=30, avg=20, median=20
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_compute_statistics_roundtrip_int16() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let (app, session_id) = app_with_session(server_key).await;

    let encrypted_list: Vec<String> = [10i16, 30, 20].iter().map(|&v| encrypt_i16_to_base64(v, client_key)).collect();
    let payload = serde_json::json!({
        "session_id": session_id,
        "encrypted_list": encrypted_list,
        "bit_width": 16,
    });

    let http_response = app.oneshot(post_to("/", &payload)).await.unwrap();
    assert_eq!(http_response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(http_response.into_body(), 50 * 1024 * 1024)
        .await
        .unwrap();
    let resp: StatisticsResponse = serde_json::from_slice(&body_bytes).unwrap();

    let sum: FheInt32 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.sum).unwrap()).unwrap();
    let min: FheInt16 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.min).unwrap()).unwrap();
    let max: FheInt16 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.max).unwrap()).unwrap();
    let avg: FheInt32 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.average).unwrap()).unwrap();
    let med: FheInt16 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.median).unwrap()).unwrap();

    let sum_val: i32 = sum.decrypt(client_key);
    let min_val: i16 = min.decrypt(client_key);
    let max_val: i16 = max.decrypt(client_key);
    let avg_val: i32 = avg.decrypt(client_key);
    let med_val: i16 = med.decrypt(client_key);
    assert_eq!(resp.count, 3, "count");
    assert_eq!(sum_val, 60, "sum");
    assert_eq!(min_val, 10, "min");
    assert_eq!(max_val, 30, "max");
    assert_eq!(avg_val, 20, "average");
    assert_eq!(med_val, 20, "median");
}

/// Roundtrip mit bit_width=8 — FheInt8 → FheInt16 Overflow-Schutz-Kette.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_compute_statistics_roundtrip_int8() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let (app, session_id) = app_with_session(server_key).await;

    let encrypted_list: Vec<String> = [10i8, 30, 20].iter().map(|&v| encrypt_i8_to_base64(v, client_key)).collect();
    let payload = serde_json::json!({
        "session_id": session_id,
        "encrypted_list": encrypted_list,
        "bit_width": 8,
    });

    let http_response = app.oneshot(post_to("/", &payload)).await.unwrap();
    assert_eq!(http_response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(http_response.into_body(), 50 * 1024 * 1024)
        .await
        .unwrap();
    let resp: StatisticsResponse = serde_json::from_slice(&body_bytes).unwrap();

    let sum: FheInt16 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.sum).unwrap()).unwrap();
    let med: FheInt8 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.median).unwrap()).unwrap();

    let sum_val: i16 = sum.decrypt(client_key);
    let med_val: i8 = med.decrypt(client_key);
    assert_eq!(resp.count, 3, "count");
    assert_eq!(sum_val, 60, "sum");
    assert_eq!(med_val, 20, "median");
}

/// Roundtrip mit bit_width=32 — FheInt32 → FheInt64 Overflow-Schutz-Kette.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_compute_statistics_roundtrip_int32() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let (app, session_id) = app_with_session(server_key).await;

    let encrypted_list: Vec<String> = [10i32, 30, 20].iter().map(|&v| encrypt_i32_to_base64(v, client_key)).collect();
    let payload = serde_json::json!({
        "session_id": session_id,
        "encrypted_list": encrypted_list,
        "bit_width": 32,
    });

    let http_response = app.oneshot(post_to("/", &payload)).await.unwrap();
    assert_eq!(http_response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(http_response.into_body(), 50 * 1024 * 1024)
        .await
        .unwrap();
    let resp: StatisticsResponse = serde_json::from_slice(&body_bytes).unwrap();

    let sum: FheInt64 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.sum).unwrap()).unwrap();
    let min: FheInt32 = bincode::deserialize(&general_purpose::STANDARD.decode(&resp.min).unwrap()).unwrap();

    let sum_val: i64 = sum.decrypt(client_key);
    let min_val: i32 = min.decrypt(client_key);
    assert_eq!(resp.count, 3, "count");
    assert_eq!(sum_val, 60, "sum");
    assert_eq!(min_val, 10, "min");
}
