use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose, Engine as _};
use serial_test::serial;
use std::sync::OnceLock;
use tfhe::prelude::*;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheInt16, FheInt32, FheInt64, FheInt8};
use tower::ServiceExt;

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

fn encrypt_i8_to_base64(plaintext_value: i8, client_key: &ClientKey) -> String {
    let encrypted = FheInt8::encrypt(plaintext_value, client_key);
    general_purpose::STANDARD.encode(bincode::serialize(&encrypted).unwrap())
}

fn encrypt_i16_to_base64(plaintext_value: i16, client_key: &ClientKey) -> String {
    let encrypted = FheInt16::encrypt(plaintext_value, client_key);
    general_purpose::STANDARD.encode(bincode::serialize(&encrypted).unwrap())
}

fn encrypt_i32_to_base64(plaintext_value: i32, client_key: &ClientKey) -> String {
    let encrypted = FheInt32::encrypt(plaintext_value, client_key);
    general_purpose::STANDARD.encode(bincode::serialize(&encrypted).unwrap())
}

fn compressed_server_key_to_base64(server_key: &CompressedServerKey) -> String {
    general_purpose::STANDARD.encode(bincode::serialize(server_key).unwrap())
}

// --- Error-Tests (kein FHE, schnell) ---

/// Ungültiges Base64 im server_key-Feld → 400
#[tokio::test]
async fn test_invalid_server_key_base64() {
    let payload = serde_json::json!({
        "encrypted_list": [],
        "server_key": "not-valid-base64!!!",
        "bit_width": 16
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Gültiges Base64, aber kein CompressedServerKey → 400
#[tokio::test]
async fn test_corrupt_server_key_bytes() {
    let payload = serde_json::json!({
        "encrypted_list": [],
        "server_key": general_purpose::STANDARD.encode(vec![1u8, 2, 3, 4, 5]),
        "bit_width": 16
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Ungültiges Base64 in einem Listenelement → 400
// multi_thread weil der Handler bei validem server_key auf
// tokio::task::block_in_place trifft — das panicked auf single-thread.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_invalid_list_item_base64() {
    let (_, server_key) = get_shared_test_key_pair();
    let payload = serde_json::json!({
        "encrypted_list": ["not-valid-base64!!!"],
        "server_key": compressed_server_key_to_base64(server_key),
        "bit_width": 16
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Gültiges Base64, aber kein FheInt16 im Listenelement → 400
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_corrupt_list_item_bytes() {
    let (_, server_key) = get_shared_test_key_pair();
    let payload = serde_json::json!({
        "encrypted_list": [general_purpose::STANDARD.encode(vec![1u8, 2, 3, 4, 5])],
        "server_key": compressed_server_key_to_base64(server_key),
        "bit_width": 16
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Leere Liste → 400
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_empty_list() {
    let (_, server_key) = get_shared_test_key_pair();
    let payload = serde_json::json!({
        "encrypted_list": [],
        "server_key": compressed_server_key_to_base64(server_key),
        "bit_width": 16
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Ungültige Bitbreite → 400
#[tokio::test]
async fn test_invalid_bit_width() {
    let (_, server_key) = get_shared_test_key_pair();
    let payload = serde_json::json!({
        "encrypted_list": [],
        "server_key": compressed_server_key_to_base64(server_key),
        "bit_width": 64
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// --- FHE Unit-Tests ---

/// Alle Statistikfunktionen auf bekannten Werten mit FheInt16 — ein einziger Test,
/// um Key-Generierung und Server-Key-Setup nur einmal zu zahlen.
///
/// Eingabe: [10, 50, 30, 20, 40] → sortiert: [10, 20, 30, 40, 50]
///   sum    = 150  (FheInt32, Overflow-Schutz)
///   count  = 5
///   min    = 10   (FheInt16)
///   max    = 50   (FheInt16)
///   average = 30  (FheInt32, 150 / 5)
///   median  = 30  (FheInt16, Index 2, mittleres Element)
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
    let encrypted_average: FheInt32 = statistics::average(&encrypted_input);

    let decrypted_sum: i32 = encrypted_sum.decrypt(client_key);
    let decrypted_min: i16 = statistics::min(&encrypted_input).decrypt(client_key);
    let decrypted_max: i16 = statistics::max(&encrypted_input).decrypt(client_key);
    let decrypted_average: i32 = encrypted_average.decrypt(client_key);
    let decrypted_median: i16 = statistics::median(&encrypted_input).decrypt(client_key);

    assert_eq!(statistics::count(&encrypted_input), 5, "count");
    assert_eq!(decrypted_sum, 150, "sum");
    assert_eq!(decrypted_min, 10, "min");
    assert_eq!(decrypted_max, 50, "max");
    assert_eq!(decrypted_average, 30, "average");
    assert_eq!(decrypted_median, 30, "median (n=5, Index 2)");
}

/// n=1 — Sonderpfad in median() (if element_count == 1) und Trivialfall für alle anderen Funktionen.
#[test]
#[serial]
fn test_statistics_single_element() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let decompressed_server_key = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(decompressed_server_key.clone()));
    tfhe::set_server_key(decompressed_server_key);

    let encrypted_single_element = vec![FheInt16::encrypt(42i16, client_key)];

    let encrypted_sum: FheInt32 = statistics::sum(&encrypted_single_element);
    let encrypted_average: FheInt32 = statistics::average(&encrypted_single_element);

    let decrypted_sum: i32 = encrypted_sum.decrypt(client_key);
    let decrypted_min: i16 = statistics::min(&encrypted_single_element).decrypt(client_key);
    let decrypted_max: i16 = statistics::max(&encrypted_single_element).decrypt(client_key);
    let decrypted_average: i32 = encrypted_average.decrypt(client_key);
    let decrypted_median: i16 = statistics::median(&encrypted_single_element).decrypt(client_key);

    assert_eq!(statistics::count(&encrypted_single_element), 1, "count");
    assert_eq!(decrypted_sum, 42, "sum");
    assert_eq!(decrypted_min, 42, "min");
    assert_eq!(decrypted_max, 42, "max");
    assert_eq!(decrypted_average, 42, "average");
    assert_eq!(decrypted_median, 42, "median");
}

/// Gerades n → Lower Median.
/// [10, 20, 30, 40] → sortiert [10, 20, 30, 40], Index (4-1)/2 = 1 → Median = 20
#[test]
#[serial]
fn test_statistics_even_n_lower_median() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let decompressed_server_key = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(decompressed_server_key.clone()));
    tfhe::set_server_key(decompressed_server_key);

    let plaintext_input = [10i16, 20, 30, 40];
    let encrypted_input: Vec<_> = plaintext_input
        .iter()
        .map(|&value| FheInt16::encrypt(value, client_key))
        .collect();

    let encrypted_average: FheInt32 = statistics::average(&encrypted_input);

    let decrypted_median: i16 = statistics::median(&encrypted_input).decrypt(client_key);
    let decrypted_average: i32 = encrypted_average.decrypt(client_key);

    assert_eq!(decrypted_median, 20, "lower median (nicht 25)");
    assert_eq!(decrypted_average, 25, "average");
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

    let plaintext_input = [-10i16, -5, 5, 10];
    let encrypted_input: Vec<_> = plaintext_input
        .iter()
        .map(|&value| FheInt16::encrypt(value, client_key))
        .collect();

    let encrypted_sum: FheInt32 = statistics::sum(&encrypted_input);
    let encrypted_average: FheInt32 = statistics::average(&encrypted_input);

    let decrypted_sum: i32 = encrypted_sum.decrypt(client_key);
    let decrypted_min: i16 = statistics::min(&encrypted_input).decrypt(client_key);
    let decrypted_max: i16 = statistics::max(&encrypted_input).decrypt(client_key);
    let decrypted_average: i32 = encrypted_average.decrypt(client_key);
    let decrypted_median: i16 = statistics::median(&encrypted_input).decrypt(client_key);

    assert_eq!(decrypted_sum, 0, "sum");
    assert_eq!(decrypted_min, -10, "min");
    assert_eq!(decrypted_max, 10, "max");
    assert_eq!(decrypted_average, 0, "average");
    assert_eq!(decrypted_median, -5, "lower median");
}

/// Average Truncation toward zero (nicht Floor).
/// [-3, -2] → sum=-5, avg=-5/2=-2 (toward zero), nicht -3 (floor)
#[test]
#[serial]
fn test_average_truncation_toward_zero() {
    let (client_key, server_key) = get_shared_test_key_pair();
    let decompressed_server_key = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(decompressed_server_key.clone()));
    tfhe::set_server_key(decompressed_server_key);

    let encrypted_input: Vec<_> = [-3i16, -2]
        .iter()
        .map(|&value| FheInt16::encrypt(value, client_key))
        .collect();

    let encrypted_average: FheInt32 = statistics::average(&encrypted_input);
    let decrypted_average: i32 = encrypted_average.decrypt(client_key);
    assert_eq!(
        decrypted_average, -2,
        "truncation toward zero: -5/2 = -2, nicht -3"
    );
}

// --- Roundtrip-Tests (HTTP) ---

/// Vollständiger HTTP-Roundtrip mit bit_width=16: Client-seitige Verschlüsselung →
/// Base64/bincode → HTTP POST → FHE-Berechnung → Entschlüsselung.
/// Fängt Integrationsfehler die in Unit-Tests unsichtbar wären,
/// z.B. inkompatible Serialisierungsformate zwischen Client und Server.
///
/// Eingabe: [10, 30, 20] → sum=60, count=3, min=10, max=30, avg=20, median=20
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_compute_statistics_roundtrip_int16() {
    let (client_key, server_key) = get_shared_test_key_pair();

    let plaintext_input = [10i16, 30, 20];
    let request_payload = StatisticsRequest {
        encrypted_list: plaintext_input
            .iter()
            .map(|&value| encrypt_i16_to_base64(value, client_key))
            .collect(),
        server_key: compressed_server_key_to_base64(server_key),
        bit_width: 16,
    };

    let http_response = create_app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(http_response.status(), StatusCode::OK);

    let response_body_bytes = axum::body::to_bytes(http_response.into_body(), 50 * 1024 * 1024)
        .await
        .unwrap();
    let statistics_response: StatisticsResponse =
        serde_json::from_slice(&response_body_bytes).unwrap();

    let encrypted_sum: FheInt32 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.sum)
            .unwrap(),
    )
    .unwrap();
    let encrypted_min: FheInt16 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.min)
            .unwrap(),
    )
    .unwrap();
    let encrypted_max: FheInt16 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.max)
            .unwrap(),
    )
    .unwrap();
    let encrypted_average: FheInt32 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.average)
            .unwrap(),
    )
    .unwrap();
    let encrypted_median: FheInt16 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.median)
            .unwrap(),
    )
    .unwrap();

    let decrypted_sum: i32 = encrypted_sum.decrypt(client_key);
    let decrypted_min: i16 = encrypted_min.decrypt(client_key);
    let decrypted_max: i16 = encrypted_max.decrypt(client_key);
    let decrypted_average: i32 = encrypted_average.decrypt(client_key);
    let decrypted_median: i16 = encrypted_median.decrypt(client_key);

    assert_eq!(statistics_response.count, 3, "count");
    assert_eq!(decrypted_sum, 60, "sum");
    assert_eq!(decrypted_min, 10, "min");
    assert_eq!(decrypted_max, 30, "max");
    assert_eq!(decrypted_average, 20, "average");
    assert_eq!(decrypted_median, 20, "median");
}

/// Roundtrip mit bit_width=8 — prüft dass die Auto-Bitbreiten-Erkennung auch für
/// kleine Werte (FheInt8 → FheInt16 für Summe/Durchschnitt) korrekt durchläuft.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_compute_statistics_roundtrip_int8() {
    let (client_key, server_key) = get_shared_test_key_pair();

    let plaintext_input = [10i8, 30, 20];
    let request_payload = StatisticsRequest {
        encrypted_list: plaintext_input
            .iter()
            .map(|&value| encrypt_i8_to_base64(value, client_key))
            .collect(),
        server_key: compressed_server_key_to_base64(server_key),
        bit_width: 8,
    };

    let http_response = create_app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(http_response.status(), StatusCode::OK);

    let response_body_bytes = axum::body::to_bytes(http_response.into_body(), 50 * 1024 * 1024)
        .await
        .unwrap();
    let statistics_response: StatisticsResponse =
        serde_json::from_slice(&response_body_bytes).unwrap();

    let encrypted_sum: FheInt16 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.sum)
            .unwrap(),
    )
    .unwrap();
    let encrypted_median: FheInt8 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.median)
            .unwrap(),
    )
    .unwrap();

    let decrypted_sum: i16 = encrypted_sum.decrypt(client_key);
    let decrypted_median: i8 = encrypted_median.decrypt(client_key);

    assert_eq!(statistics_response.count, 3, "count");
    assert_eq!(decrypted_sum, 60, "sum");
    assert_eq!(decrypted_median, 20, "median");
}

/// Roundtrip mit bit_width=32 — prüft die FheInt32 → FheInt64 Overflow-Schutz-Kette.
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_compute_statistics_roundtrip_int32() {
    let (client_key, server_key) = get_shared_test_key_pair();

    let plaintext_input = [10i32, 30, 20];
    let request_payload = StatisticsRequest {
        encrypted_list: plaintext_input
            .iter()
            .map(|&value| encrypt_i32_to_base64(value, client_key))
            .collect(),
        server_key: compressed_server_key_to_base64(server_key),
        bit_width: 32,
    };

    let http_response = create_app()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(http_response.status(), StatusCode::OK);

    let response_body_bytes = axum::body::to_bytes(http_response.into_body(), 50 * 1024 * 1024)
        .await
        .unwrap();
    let statistics_response: StatisticsResponse =
        serde_json::from_slice(&response_body_bytes).unwrap();

    let encrypted_sum: FheInt64 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.sum)
            .unwrap(),
    )
    .unwrap();
    let encrypted_min: FheInt32 = bincode::deserialize(
        &general_purpose::STANDARD
            .decode(&statistics_response.min)
            .unwrap(),
    )
    .unwrap();

    let decrypted_sum: i64 = encrypted_sum.decrypt(client_key);
    let decrypted_min: i32 = encrypted_min.decrypt(client_key);

    assert_eq!(statistics_response.count, 3, "count");
    assert_eq!(decrypted_sum, 60, "sum");
    assert_eq!(decrypted_min, 10, "min");
}

// --- Hilfsfunktion ---

fn post_json(payload: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/")
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(payload).unwrap()))
        .unwrap()
}
