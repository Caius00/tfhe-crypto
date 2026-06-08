//! Integrationstests gegen den vollständigen Router.
//!
//! Brauchen ein laufendes Redis auf 127.0.0.1:6379. Lokal am bequemsten:
//!   `docker run --rm -p 6379:6379 redis:7`
//!
//! Die Tests laufen seriell (`#[serial]`), weil sie sich denselben Redis-Slot
//! teilen und mit `/clear` aufräumen — gleichzeitig hätten sie sich gegenseitig
//! Einträge wegputzen können.

use aide::axum::routing::post_with;
use aide::axum::ApiRouter;
use axum::extract::DefaultBodyLimit;
use axum_test::TestServer;
use base64::{engine::general_purpose, Engine as _};
use encrypted_key_value_store::custom_fhe_ascii_string::{
    CompressedCustomFheAsciiString, CustomFheAsciiString,
};
use encrypted_key_value_store::models::{
    ClearRequest, CreateSessionRequest, CreateSessionResponse, ExistsRequest, ExistsResponse,
    GetRequest, MessageResponse, PutRequest, ValueResponse,
};
use encrypted_key_value_store::routes::{
    clear_entries, create_session, entry_exists, get_entry, put_entry,
};
use encrypted_key_value_store::store::{AppState, SharedState};
use serial_test::serial;
use std::sync::Arc;
use tfhe::prelude::FheDecrypt;
use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
use tfhe::{set_server_key, ClientKey, CompressedServerKey, FheBool};

/// Baut die Test-App. `axum_test::TestServer` führt den Router in-process aus —
/// keine Netzwerkverbindung nötig.
fn make_server() -> TestServer {
    let state: SharedState = Arc::new(AppState::new());
    let api = ApiRouter::new()
        .api_route("/session", post_with(create_session, |op| op))
        .api_route("/entry", post_with(put_entry, |op| op))
        .api_route("/entry/get", post_with(get_entry, |op| op))
        .api_route("/entry/exists", post_with(entry_exists, |op| op))
        .api_route("/clear", post_with(clear_entries, |op| op))
        .with_state(state);

    let router: axum::Router = api.into();
    TestServer::new(router.layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024)))
}

/// Erzeugt einen frischen Client-Key und setzt den dazugehörigen Server-Key
/// als Thread-lokalen Default — bequem für die Klartext-Verschlüsselung im Test.
fn fresh_keys() -> (ClientKey, CompressedServerKey) {
    let params =
        MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
            .with_compression(true)
            .find()
            .expect("could not find suitable parameters");
    let client_key = ClientKey::generate(params);
    let server_key = CompressedServerKey::new(&client_key);
    set_server_key(server_key.clone().decompress());
    (client_key, server_key)
}

/// Verschlüsselt einen Klartext-String zu Base64-Chunks (= Wire-Format).
fn encrypt_chunks(s: &str, client_key: &ClientKey) -> Vec<String> {
    CustomFheAsciiString::new(s, client_key)
        .compress()
        .unwrap()
        .chunks
        .iter()
        .map(|c| general_purpose::STANDARD.encode(c))
        .collect()
}

/// Entschlüsselt Base64-Chunks zurück zum Klartext-String.
fn decrypt_chunks(chunks: &[String], client_key: &ClientKey) -> String {
    let bytes: Vec<Vec<u8>> = chunks
        .iter()
        .map(|c| general_purpose::STANDARD.decode(c).unwrap())
        .collect();
    CompressedCustomFheAsciiString::from_chunks(bytes)
        .decompress()
        .unwrap()
        .decrypt(client_key)
}

/// Hilfsfunktion: Session am Server registrieren und ID zurückgeben.
async fn open_session(server: &TestServer, server_key: &CompressedServerKey) -> String {
    let bytes = bincode::serialize(server_key).unwrap();
    let body = CreateSessionRequest {
        server_key: general_purpose::STANDARD.encode(&bytes),
    };
    let resp = server.post("/session").json(&body).await;
    resp.assert_status_ok();
    resp.json::<CreateSessionResponse>().session_id
}

async fn put(
    server: &TestServer,
    session_id: &str,
    client_key: &ClientKey,
    key: &str,
    value: &str,
    ttl: Option<u64>,
) {
    let body = PutRequest {
        session_id: session_id.to_string(),
        key: encrypt_chunks(key, client_key),
        value: encrypt_chunks(value, client_key),
        ttl_seconds: ttl,
    };
    let resp = server.post("/entry").json(&body).await;
    resp.assert_status_ok();
    let _: MessageResponse = resp.json();
}

async fn get(server: &TestServer, session_id: &str, client_key: &ClientKey, key: &str) -> String {
    let body = GetRequest {
        session_id: session_id.to_string(),
        key: encrypt_chunks(key, client_key),
    };
    let resp = server.post("/entry/get").json(&body).await;
    resp.assert_status_ok();
    let value_resp: ValueResponse = resp.json();
    decrypt_chunks(&value_resp.value, client_key)
}

async fn exists(server: &TestServer, session_id: &str, client_key: &ClientKey, key: &str) -> bool {
    let body = ExistsRequest {
        session_id: session_id.to_string(),
        key: encrypt_chunks(key, client_key),
    };
    let resp = server.post("/entry/exists").json(&body).await;
    resp.assert_status_ok();
    let exists_resp: ExistsResponse = resp.json();
    let bytes = general_purpose::STANDARD
        .decode(&exists_resp.exists)
        .unwrap();
    // Server schickt einen unkomprimierten FheBool — wir haben homomorph
    // OR-akkumuliert und nicht wieder komprimiert.
    let enc: FheBool = bincode::deserialize(&bytes).unwrap();
    enc.decrypt(client_key)
}

async fn clear(server: &TestServer, session_id: &str) {
    let body = ClearRequest {
        session_id: session_id.to_string(),
    };
    let resp = server.post("/clear").json(&body).await;
    resp.assert_status_ok();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn put_then_get_returns_same_value() {
    let server = make_server();
    let (client_key, server_key) = fresh_keys();
    let sid = open_session(&server, &server_key).await;
    clear(&server, &sid).await;

    put(&server, &sid, &client_key, "name", "Jannes", None).await;
    let got = get(&server, &sid, &client_key, "name").await;
    assert_eq!(got, "Jannes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn exists_is_false_before_put_and_true_after() {
    let server = make_server();
    let (client_key, server_key) = fresh_keys();
    let sid = open_session(&server, &server_key).await;
    clear(&server, &sid).await;

    put(&server, &sid, &client_key, "alpha", "first", None).await;

    assert!(exists(&server, &sid, &client_key, "alpha").await);
    assert!(!exists(&server, &sid, &client_key, "beta").await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn clear_only_affects_own_session() {
    let server = make_server();
    let (ck_a, sk_a) = fresh_keys();
    let sid_a = open_session(&server, &sk_a).await;
    let (ck_b, sk_b) = fresh_keys();
    // Server-Key auf den B-Key setzen, damit die Put-Verschlüsselung von B
    // mit dem im Tests-Thread aktiven Key passt. (TestServer ist in-process.)
    set_server_key(sk_b.clone().decompress());
    let sid_b = open_session(&server, &sk_b).await;

    set_server_key(sk_a.clone().decompress());
    put(&server, &sid_a, &ck_a, "shared", "valueA", None).await;
    set_server_key(sk_b.clone().decompress());
    put(&server, &sid_b, &ck_b, "shared", "valueB", None).await;

    set_server_key(sk_a.clone().decompress());
    clear(&server, &sid_a).await;

    // B-Eintrag muss noch da sein.
    set_server_key(sk_b.decompress());
    assert!(exists(&server, &sid_b, &ck_b, "shared").await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn unknown_session_is_unauthorized() {
    let server = make_server();
    let (client_key, _server_key) = fresh_keys();

    let body = PutRequest {
        session_id: "00000000-0000-0000-0000-000000000000".to_string(),
        key: encrypt_chunks("k", &client_key),
        value: encrypt_chunks("v", &client_key),
        ttl_seconds: None,
    };
    let resp = server.post("/entry").json(&body).await;
    resp.assert_status_unauthorized();
}
