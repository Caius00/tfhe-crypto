use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post};
use axum::Router;
use axum_test::TestServer;
use encrypted_key_value_store::custom_fhe_ascii_string::{
    CompressedCustomFheAsciiString, CustomFheAsciiString,
};
use encrypted_key_value_store::models::{
    CreateSessionRequest, DeleteRequest, ExistsRequest, GetRequest, MessageResponse, PutRequest,
    ValueResponse,
};
use encrypted_key_value_store::routes::{
    clear_db, create_session_route, delete_route, exists_route, get_route, put_route,
};
use encrypted_key_value_store::store::{AppState, SharedState};
use std::sync::Arc;
use tfhe::prelude::FheDecrypt;
use tfhe::{ClientKey, CompressedFheBool, CompressedServerKey};

async fn get_session_id(
    server: &TestServer,
    compressed_server_key: &CompressedServerKey,
) -> String {
    let serialized_server_key = bincode::serialize(&compressed_server_key).unwrap();
    session_req(&server, serialized_server_key).await
}

fn get_test_server() -> TestServer {
    let state: SharedState = Arc::new(AppState::new());
    let app = Router::new()
        .route("/session", post(create_session_route))
        .route("/entry", post(put_route))
        .route("/entry", get(get_route))
        .route("/entry/exists", get(exists_route))
        .route("/entry", delete(delete_route))
        .route("/clear", delete(clear_db))
        .with_state(state)
        .layer(DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    TestServer::new(app)
}

async fn session_req(server: &TestServer, compressed_server_key: Vec<u8>) -> String {
    let session_response = server
        .post("/session")
        .json(&CreateSessionRequest {
            server_key: compressed_server_key,
        })
        .await;
    session_response.json::<MessageResponse>().message
}

async fn put_req(
    server: &TestServer,
    enc_key: &CustomFheAsciiString,
    enc_value: &CustomFheAsciiString,
    session_id: &str,
) {
    let response = server
        .post("/entry")
        .json(&PutRequest {
            key: enc_key.compress().string,
            value: enc_value.compress().string,
            session_id: session_id.to_string(),
        })
        .await;

    response.assert_status_ok();
}

async fn get_req(
    server: &TestServer,
    enc_key: &CustomFheAsciiString,
    session_id: &String,
    client_key: &ClientKey,
) -> String {
    let response = server
        .get("/entry")
        .json(&GetRequest {
            key: enc_key.compress().string,
            session_id: session_id.clone(),
        })
        .await;

    response.assert_status_ok();
    let body = response.json::<ValueResponse>();

    let response_value: Vec<u8> = body.value;
    CompressedCustomFheAsciiString::new(response_value)
        .decompress()
        .decrypt(&client_key)
}

async fn exists_req(
    server: &TestServer,
    enc_key: &CustomFheAsciiString,
    session_id: &String,
    client_key: &ClientKey,
) -> bool {
    let response = server
        .get("/entry/exists")
        .json(&ExistsRequest {
            key: enc_key.compress().string,
            session_id: session_id.clone(),
        })
        .await;

    response.assert_status_ok();
    let body = response.json::<ValueResponse>();

    let response_value: CompressedFheBool = bincode::deserialize(&body.value).unwrap();
    response_value.decompress().decrypt(client_key)
}

async fn delete_req(server: &TestServer, enc_key: &CustomFheAsciiString, session_id: &String) {
    let response = server
        .delete("/entry")
        .json(&DeleteRequest {
            key: enc_key.compress().string,
            session_id: session_id.clone(),
        })
        .await;

    response.assert_status_ok();
}

#[cfg(test)]
mod test_set {
    use super::*;
    use encrypted_key_value_store::custom_fhe_ascii_string::CustomFheAsciiString;
    use std::time::Duration;
    use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
    use tfhe::{set_server_key};
    use tokio::time::sleep;

    async fn run_client(server: Arc<TestServer>, key: &str, value: &str) {
        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);

        let session_id = get_session_id(&server, &compressed_server_key).await;
        put_req(&server, &enc_key, &enc_value, &session_id).await;
    }

    #[tokio::test]
    async fn basic() {
        let server = get_test_server();

        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let key = "Hello Key";
        let value = "Hello Value";
        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);

        let session_id = get_session_id(&server, &compressed_server_key).await;
        put_req(&server, &enc_key, &enc_value, &session_id).await;
    }

    #[tokio::test]
    pub async fn different_threads() {
        let server = Arc::new(get_test_server());

        sleep(Duration::from_millis(100)).await;

        let s1 = Arc::clone(&server);
        let s2 = Arc::clone(&server);

        let c1 = tokio::spawn(run_client(s1, "Hello Key A", "Hello Value A"));
        let c2 = tokio::spawn(run_client(s2, "Hello Key B", "Hello Value B"));

        tokio::try_join!(c1, c2).unwrap();
    }

    #[tokio::test]
    async fn different_threads_same_key() {
        let server = Arc::new(get_test_server());

        sleep(Duration::from_millis(100)).await;

        let s1 = Arc::clone(&server);
        let s2 = Arc::clone(&server);

        let c1 = tokio::spawn(run_client(s1, "Hello Key", "Hello Value A"));
        let c2 = tokio::spawn(run_client(s2, "Hello Key", "Hello Value B"));

        tokio::try_join!(c1, c2).unwrap();
    }

    #[tokio::test]
    async fn empty_strings() {
        let server = Arc::new(get_test_server());

        sleep(Duration::from_millis(100)).await;

        let s1 = Arc::clone(&server);
        let s2 = Arc::clone(&server);

        let c1 = tokio::spawn(run_client(s1, "", ""));
        let c2 = tokio::spawn(run_client(s2, "", "Hello Value B"));

        tokio::try_join!(c1, c2).unwrap();
    }

    #[tokio::test]
    async fn non_utf8() {
        let server = Arc::new(get_test_server());

        sleep(Duration::from_millis(100)).await;

        let s1 = Arc::clone(&server);
        let s2 = Arc::clone(&server);

        let c1 = tokio::spawn(run_client(s1, "你好世界", "你好，价值"));
        let c2 = tokio::spawn(run_client(s2, "你好世界", "你好，价值"));

        tokio::try_join!(c1, c2).unwrap();
    }
}

#[cfg(test)]
mod test_get {
    use super::*;
    use encrypted_key_value_store::custom_fhe_ascii_string::CustomFheAsciiString;
    use std::time::Duration;
    use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
    use tfhe::{set_server_key, CompressedServerKey};
    use tokio::time::sleep;

    async fn run_client(server: Arc<TestServer>, key: &str, value: &str) {
        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);

        let session_id = get_session_id(&server, &compressed_server_key).await;
        put_req(&server, &enc_key, &enc_value, &session_id).await;
        sleep(Duration::from_millis(100)).await;

        let found = get_req(&server, &enc_key, &session_id, &client_key).await;

        assert_eq!(found, value.to_string());
    }

    #[tokio::test]
    async fn basic() {
        let server = get_test_server();

        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = tfhe::ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let key = "Hello Key";
        let value = "Hello Value";
        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);

        // Setup
        let session_id = get_session_id(&server, &compressed_server_key).await;

        put_req(&server, &enc_key, &enc_value, &session_id).await;

        let response_value = get_req(&server, &enc_value, &session_id, &client_key).await;
        assert_eq!(value, response_value);
    }

    #[tokio::test]
    async fn two_clients() {
        let server = Arc::new(get_test_server());

        sleep(Duration::from_millis(100)).await;

        let s1 = Arc::clone(&server);
        let s2 = Arc::clone(&server);

        let c1 = tokio::spawn(run_client(s1, "Hello Key A", "Hello Value A"));
        sleep(Duration::from_secs(30)).await;
        let c2 = tokio::spawn(run_client(s2, "Hello Key B", "Hello Value B"));

        tokio::try_join!(c1, c2).unwrap();
    }
}

#[cfg(test)]
mod test_exists {
    use super::*;
    use encrypted_key_value_store::custom_fhe_ascii_string::CustomFheAsciiString;
    use serial_test::serial;
    use std::time::Duration;
    use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
    use tfhe::{set_server_key, CompressedServerKey};
    use tokio::time::sleep;

    async fn run_client(
        server: Arc<TestServer>,
        key: &str,
        value: &str,
        insert_before: bool,
    ) -> bool {
        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);

        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);
        let session_id = get_session_id(&server, &compressed_server_key).await;
        if insert_before {
            put_req(&server, &enc_key, &enc_value, &session_id).await;
            sleep(Duration::from_millis(100)).await;
        }

        exists_req(&server, &enc_key, &session_id, &client_key).await
    }

    #[tokio::test]
    async fn basic() {
        let server = get_test_server();

        let parameters =
            MetaParametersFinder::new(Constraint::LessThanOrEqual(Log2PFail(-128.0)), Backend::Cpu)
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

        let client_key = tfhe::ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let key = "Hello Key";
        let value = "Hello Value";
        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);

        // Setup
        server.delete("/clear").await;
        let session_id = get_session_id(&server, &compressed_server_key).await;

        put_req(&server, &enc_key, &enc_value, &session_id).await;

        // Test
        let first_exists = exists_req(&server, &enc_key, &session_id, &client_key).await;
        assert!(first_exists);

        server.delete("/clear").await;

        let second_exists = exists_req(&server, &enc_key, &session_id, &client_key).await;
        assert!(!second_exists);
    }

    #[tokio::test]
    #[serial]
    async fn two_clients_different_key() {
        let server = Arc::new(get_test_server());

        sleep(Duration::from_millis(100)).await;

        let s1 = Arc::clone(&server);
        let s2 = Arc::clone(&server);

        let c1 = tokio::spawn(run_client(s1, "Hello Key A", "Hello Value A", true));
        let c2 = tokio::spawn(run_client(s2, "Hello Key B", "Hello Value B", false));

        let results = tokio::try_join!(c1, c2).unwrap();

        println!("{:?}", results);

        assert!(results.0);
        assert!(!results.1);
    }

    #[cfg(test)]
    mod test_delete {
        use super::*;
        use encrypted_key_value_store::custom_fhe_ascii_string::CustomFheAsciiString;
        use tfhe::set_server_key;
        use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};

        #[tokio::test]
        async fn test_delete_route() {
            let server = get_test_server();

            let parameters = MetaParametersFinder::new(
                Constraint::LessThanOrEqual(Log2PFail(-128.0)),
                Backend::Cpu,
            )
                .with_compression(true)
                .find()
                .expect("Could not find suitable parameters");

            let client_key = tfhe::ClientKey::generate(parameters);
            let compressed_server_key = CompressedServerKey::new(&client_key);
            set_server_key(compressed_server_key.decompress());

            let key = "Hello Key";
            let value = "Hello Value";
            let enc_key = CustomFheAsciiString::new(key, &client_key);
            let enc_value = CustomFheAsciiString::new(value, &client_key);

            server.delete("/clear").await;
            let session_id = get_session_id(&server, &compressed_server_key).await;
            put_req(&server, &enc_key, &enc_value, &session_id).await;
            let session_id = get_session_id(&server, &compressed_server_key).await;

            delete_req(&server, &enc_key, &session_id).await;

            let exists = exists_req(&server, &enc_key, &session_id, &client_key).await;
            assert!(!exists);
        }
    }
}

#[cfg(test)]
mod test_all {
    use super::*;
    use encrypted_key_value_store::custom_fhe_ascii_string::CustomFheAsciiString;
    use serial_test::serial;
    use std::time::Duration;
    use tfhe::set_server_key;
    use tfhe::shortint::parameters::{Backend, Constraint, Log2PFail, MetaParametersFinder};
    use tokio::time::sleep;

    async fn test_all(
        server: &TestServer,
        compressed_server_key: CompressedServerKey,
        enc_key: CustomFheAsciiString,
        enc_value: CustomFheAsciiString,
        value: &str,
        client_key: &ClientKey,
    ) {
        // Setup
        let session_id = get_session_id(server, &compressed_server_key).await;

        // Check initial exists
        let initial_exists = exists_req(server, &enc_key, &session_id, client_key).await;
        assert!(!initial_exists);

        // Put
        put_req(server, &enc_key, &enc_value, &session_id).await;

        // Check Successful Put
        let put_exists = exists_req(server, &enc_key, &session_id, client_key).await;
        assert!(put_exists);
        println!("Passed Put test.");

        // Get
        let response_value = get_req(server, &enc_value, &session_id, client_key).await;
        assert_eq!(value, response_value);
        println!("Passed Get test.");

        // Delete
        delete_req(server, &enc_key, &session_id).await;

        // Check Successful Delete
        let delete_exists = exists_req(server, &enc_key, &session_id, client_key).await;
        assert!(!delete_exists);
        println!("Passed Delete test.");
    }

    async fn run_client(server: Arc<TestServer>, key: &str, value: &str) {
        let parameters = MetaParametersFinder::new(
            Constraint::LessThanOrEqual(Log2PFail(-128.0)),
            Backend::Cpu,
        )
        .with_compression(true)
        .find()
        .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);

        test_all(
            &server,
            compressed_server_key,
            enc_key,
            enc_value,
            value,
            &client_key,
        )
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn basic() {
        let server = get_test_server();
        server.delete("/clear").await;

        let parameters = MetaParametersFinder::new(
            Constraint::LessThanOrEqual(Log2PFail(-128.0)),
            Backend::Cpu,
        )
        .with_compression(true)
        .find()
        .expect("Could not find suitable parameters");

        let client_key = ClientKey::generate(parameters);
        let compressed_server_key = CompressedServerKey::new(&client_key);
        set_server_key(compressed_server_key.decompress());

        let key = "Hello Key";
        let value = "Hello Value";
        let enc_key = CustomFheAsciiString::new(key, &client_key);
        let enc_value = CustomFheAsciiString::new(value, &client_key);

        test_all(
            &server,
            compressed_server_key,
            enc_key,
            enc_value,
            value,
            &client_key,
        )
        .await;
    }

    #[tokio::test]
    #[serial]
    async fn two_clients() {
        let server = Arc::new(get_test_server());
        server.delete("/clear").await;

        sleep(Duration::from_millis(100)).await;

        let s1 = Arc::clone(&server);
        let s2 = Arc::clone(&server);

        let c1 = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(run_client(s1, "Hello Key A", "Hello Value A"))
        });

        let c2 = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(run_client(s2, "Hello Key B", "Hello Value B"))
        });

        c1.join().unwrap();
        c2.join().unwrap();
    }
}
