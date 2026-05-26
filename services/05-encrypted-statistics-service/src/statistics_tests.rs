use super::*;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use base64::{engine::general_purpose, Engine as _};
use serial_test::serial;
use std::sync::OnceLock;
use tfhe::prelude::*;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheInt32, FheInt64};
use tower::ServiceExt;

// Keys werden einmalig für alle FHE-Tests generiert.
static TEST_SETUP: OnceLock<(ClientKey, CompressedServerKey)> = OnceLock::new();

fn get_test_setup() -> &'static (ClientKey, CompressedServerKey) {
    TEST_SETUP.get_or_init(|| {
        let config = ConfigBuilder::default().build();
        let ck = ClientKey::generate(config);
        let sk = CompressedServerKey::new(&ck);
        (ck, sk)
    })
}

fn encrypt_i32(val: i32, ck: &ClientKey) -> String {
    let enc = FheInt32::encrypt(val, ck);
    general_purpose::STANDARD.encode(bincode::serialize(&enc).unwrap())
}

fn server_key_b64(sk: &CompressedServerKey) -> String {
    general_purpose::STANDARD.encode(bincode::serialize(sk).unwrap())
}

// --- Error-Tests (kein FHE, schnell) ---

/// Ungültiges Base64 im server_key-Feld → 400
#[tokio::test]
async fn test_invalid_server_key_base64() {
    let payload = serde_json::json!({
        "encrypted_list": [],
        "server_key": "not-valid-base64!!!"
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Gültiges Base64, aber kein CompressedServerKey → 400
#[tokio::test]
async fn test_corrupt_server_key_bytes() {
    let payload = serde_json::json!({
        "encrypted_list": [],
        "server_key": general_purpose::STANDARD.encode(vec![1u8, 2, 3, 4, 5])
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Ungültiges Base64 in einem Listenelement → 400
#[tokio::test]
async fn test_invalid_list_item_base64() {
    let (_, sk) = get_test_setup();
    let payload = serde_json::json!({
        "encrypted_list": ["not-valid-base64!!!"],
        "server_key": server_key_b64(sk)
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Gültiges Base64, aber kein FheInt32 im Listenelement → 400
#[tokio::test]
async fn test_corrupt_list_item_bytes() {
    let (_, sk) = get_test_setup();
    let payload = serde_json::json!({
        "encrypted_list": [general_purpose::STANDARD.encode(vec![1u8, 2, 3, 4, 5])],
        "server_key": server_key_b64(sk)
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Leere Liste → 400
#[tokio::test]
async fn test_empty_list() {
    let (_, sk) = get_test_setup();
    let payload = serde_json::json!({
        "encrypted_list": [],
        "server_key": server_key_b64(sk)
    });
    let response = create_app().oneshot(post_json(&payload)).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// --- FHE Unit-Test ---

/// Alle Statistikfunktionen auf bekannten Werten — ein einziger Test,
/// um Key-Generierung und Server-Key-Setup nur einmal zu zahlen.
///
/// Eingabe: [10, 50, 30, 20, 40] → sortiert: [10, 20, 30, 40, 50]
///   sum    = 150
///   count  = 5
///   min    = 10
///   max    = 50
///   average = 30  (150 / 5)
///   median  = 30  (Index 2, mittleres Element)
#[test]
#[serial]
fn test_statistics_functions() {
    let (client_key, server_key) = get_test_setup();
    let sk = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(sk.clone()));
    tfhe::set_server_key(sk);

    let input = [10i32, 50, 30, 20, 40];
    let enc: Vec<FheInt32> = input
        .iter()
        .map(|&x| FheInt32::encrypt(x, client_key))
        .collect();

    let sum_val: i64 = statistics::sum(&enc).decrypt(client_key);
    assert_eq!(sum_val, 150, "sum");

    assert_eq!(statistics::count(&enc), 5, "count");

    let min_val: i32 = statistics::min(&enc).decrypt(client_key);
    assert_eq!(min_val, 10, "min");

    let max_val: i32 = statistics::max(&enc).decrypt(client_key);
    assert_eq!(max_val, 50, "max");

    let avg_val: i64 = statistics::average(&enc).decrypt(client_key);
    assert_eq!(avg_val, 30, "average");

    let median_val: i32 = statistics::median(&enc).decrypt(client_key);
    assert_eq!(median_val, 30, "median (n=5, Index 2)");
}

/// n=1 — Sonderpfad in median() (if n == 1) und Trivialfall für alle anderen Funktionen.
#[test]
#[serial]
fn test_statistics_single_element() {
    let (client_key, server_key) = get_test_setup();
    let sk = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(sk.clone()));
    tfhe::set_server_key(sk);

    let enc = vec![FheInt32::encrypt(42i32, client_key)];

    let sum_val: i64 = statistics::sum(&enc).decrypt(client_key);
    let min_val: i32 = statistics::min(&enc).decrypt(client_key);
    let max_val: i32 = statistics::max(&enc).decrypt(client_key);
    let avg_val: i64 = statistics::average(&enc).decrypt(client_key);
    let median_val: i32 = statistics::median(&enc).decrypt(client_key);

    assert_eq!(statistics::count(&enc), 1, "count");
    assert_eq!(sum_val, 42, "sum");
    assert_eq!(min_val, 42, "min");
    assert_eq!(max_val, 42, "max");
    assert_eq!(avg_val, 42, "average");
    assert_eq!(median_val, 42, "median");
}

/// Gerades n → Lower Median.
/// [10, 20, 30, 40] → sortiert [10, 20, 30, 40], Index (4-1)/2 = 1 → Median = 20
#[test]
#[serial]
fn test_statistics_even_n_lower_median() {
    let (client_key, server_key) = get_test_setup();
    let sk = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(sk.clone()));
    tfhe::set_server_key(sk);

    let input = [10i32, 20, 30, 40];
    let enc: Vec<FheInt32> = input
        .iter()
        .map(|&x| FheInt32::encrypt(x, client_key))
        .collect();

    let median_val: i32 = statistics::median(&enc).decrypt(client_key);
    let avg_val: i64 = statistics::average(&enc).decrypt(client_key);

    assert_eq!(median_val, 20, "lower median (nicht 25)");
    assert_eq!(avg_val, 25, "average");
}

/// Negative Eingabewerte — FheInt32 unterstützt Vorzeichen.
/// [-10, -5, 5, 10] → sum=0, min=-10, max=10, avg=0, median=-5
#[test]
#[serial]
fn test_statistics_negative_values() {
    let (client_key, server_key) = get_test_setup();
    let sk = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(sk.clone()));
    tfhe::set_server_key(sk);

    let input = [-10i32, -5, 5, 10];
    let enc: Vec<FheInt32> = input
        .iter()
        .map(|&x| FheInt32::encrypt(x, client_key))
        .collect();

    let sum_val: i64 = statistics::sum(&enc).decrypt(client_key);
    let min_val: i32 = statistics::min(&enc).decrypt(client_key);
    let max_val: i32 = statistics::max(&enc).decrypt(client_key);
    let avg_val: i64 = statistics::average(&enc).decrypt(client_key);
    let median_val: i32 = statistics::median(&enc).decrypt(client_key);

    assert_eq!(sum_val, 0, "sum");
    assert_eq!(min_val, -10, "min");
    assert_eq!(max_val, 10, "max");
    assert_eq!(avg_val, 0, "average");
    assert_eq!(median_val, -5, "lower median");
}

/// Average Truncation toward zero (nicht Floor).
/// [-3, -2] → sum=-5, avg=-5/2=-2 (toward zero), nicht -3 (floor)
#[test]
#[serial]
fn test_average_truncation_toward_zero() {
    let (client_key, server_key) = get_test_setup();
    let sk = server_key.decompress();
    rayon::broadcast(|_| tfhe::set_server_key(sk.clone()));
    tfhe::set_server_key(sk);

    let enc: Vec<FheInt32> = [-3i32, -2]
        .iter()
        .map(|&x| FheInt32::encrypt(x, client_key))
        .collect();

    let avg_val: i64 = statistics::average(&enc).decrypt(client_key);
    assert_eq!(avg_val, -2, "truncation toward zero: -5/2 = -2, nicht -3");
}

// --- Roundtrip-Test ---

/// Vollständiger HTTP-Roundtrip: Client-seitige Verschlüsselung →
/// Base64/bincode → HTTP POST → FHE-Berechnung → Entschlüsselung.
/// Fängt Integrationsfehler die in Unit-Tests unsichtbar wären,
/// z.B. inkompatible Serialisierungsformate zwischen Client und Server.
///
/// Eingabe: [10, 30, 20] → sum=60, count=3, min=10, max=30, avg=20, median=20
#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn test_compute_statistics_roundtrip() {
    let (client_key, server_key) = get_test_setup();

    let input = [10i32, 30, 20];
    let payload = StatisticsRequest {
        encrypted_list: input.iter().map(|&x| encrypt_i32(x, client_key)).collect(),
        server_key: server_key_b64(server_key),
    };

    let response = create_app()
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

    let body = axum::body::to_bytes(response.into_body(), 50 * 1024 * 1024)
        .await
        .unwrap();
    let res: StatisticsResponse = serde_json::from_slice(&body).unwrap();

    let sum_enc: FheInt64 =
        bincode::deserialize(&general_purpose::STANDARD.decode(&res.sum).unwrap()).unwrap();
    let min_enc: FheInt32 =
        bincode::deserialize(&general_purpose::STANDARD.decode(&res.min).unwrap()).unwrap();
    let max_enc: FheInt32 =
        bincode::deserialize(&general_purpose::STANDARD.decode(&res.max).unwrap()).unwrap();
    let avg_enc: FheInt64 =
        bincode::deserialize(&general_purpose::STANDARD.decode(&res.average).unwrap()).unwrap();
    let median_enc: FheInt32 =
        bincode::deserialize(&general_purpose::STANDARD.decode(&res.median).unwrap()).unwrap();

    let sum_val: i64 = sum_enc.decrypt(client_key);
    let min_val: i32 = min_enc.decrypt(client_key);
    let max_val: i32 = max_enc.decrypt(client_key);
    let avg_val: i64 = avg_enc.decrypt(client_key);
    let median_val: i32 = median_enc.decrypt(client_key);

    assert_eq!(res.count, 3, "count");
    assert_eq!(sum_val, 60, "sum");
    assert_eq!(min_val, 10, "min");
    assert_eq!(max_val, 30, "max");
    assert_eq!(avg_val, 20, "average");
    assert_eq!(median_val, 20, "median");
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
