#[cfg(test)]
mod integration_tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use base64::{engine::general_purpose, Engine as _};
    use serde_json::{json, Value};
    use tfhe::{generate_keys, prelude::*, CompressedServerKey, ConfigBuilder, FheBool, FheUint8};
    use tower::util::ServiceExt;

    use encrypted_voting_polling::voting::logic::{
        approve_participant, create_session, get_pending, get_results, join_session, submit_vote,
    };
    use encrypted_voting_polling::voting::types::AppState;

    fn build_app() -> (Router, AppState) {
        let state: AppState = Arc::new(Mutex::new(HashMap::new()));
        let app = Router::new()
            .route("/session", post(create_session))
            .route("/join", post(join_session))
            .route("/pending/{session_id}/{creator_id}", get(get_pending))
            .route("/approve", post(approve_participant))
            .route("/vote", post(submit_vote))
            .route("/results/{session_id}/{creator_id}", get(get_results))
            .with_state(state.clone())
            .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));
        (app, state)
    }

    async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()));
        (status, json)
    }

    async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).to_string()));
        (status, json)
    }

    fn generate_fhe_keys() -> (tfhe::ClientKey, String) {
        let config = ConfigBuilder::default().build();
        let (client_key, _) = generate_keys(config);
        let compressed = CompressedServerKey::new(&client_key);
        let sk_bytes = bincode::serialize(&compressed).unwrap();
        let sk_b64 = general_purpose::STANDARD.encode(&sk_bytes);
        (client_key, sk_b64)
    }

    fn encrypt_bool(value: bool, client_key: &tfhe::ClientKey) -> String {
        let encrypted = FheBool::encrypt(value, client_key);
        let bytes = bincode::serialize(&encrypted).unwrap();
        general_purpose::STANDARD.encode(&bytes)
    }

    fn encrypt_uint8(value: u8, client_key: &tfhe::ClientKey) -> String {
        let encrypted = FheUint8::encrypt(value, client_key);
        let bytes = bincode::serialize(&encrypted).unwrap();
        general_purpose::STANDARD.encode(&bytes)
    }

    fn decrypt_uint8(b64: &str, client_key: &tfhe::ClientKey) -> u8 {
        let bytes = general_purpose::STANDARD.decode(b64).unwrap();
        let encrypted: FheUint8 = bincode::deserialize(&bytes).unwrap();
        encrypted.decrypt(client_key)
    }

    // =========================================================================
    // TEST 1: Bool-Voting-Flow
    // =========================================================================
    // TODO: FheBool encryption not compatible with FheUint8 aggregate — needs rework
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn test_bool_voting_full_flow() {
        let (app, _state) = build_app();
        let (client_key, sk_b64) = generate_fhe_keys();

        // 1. Session erstellen
        let (status, body) = post_json(
            &app,
            "/session",
            json!({
                "creator_id": "alice",
                "server_key": sk_b64,
                "questions": [{
                    "id": 1,
                    "text": "Soll das Projekt fortgesetzt werden?",
                    "question_type": "bool",
                    "options": null,
                    "multiple": null
                }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "Session erstellen fehlgeschlagen");
        let session_id = body["session_id"].as_str().unwrap().to_string();
        println!("Session erstellt: {}", session_id);

        // 2. Teilnehmer beitreten – mit optionalem enc_name_chunks
        for participant in ["bob", "carol"] {
            let (status, body) = post_json(
                &app,
                "/join",
                json!({
                    "session_id": session_id,
                    "participant_id": participant,
                    "enc_name_chunks": null
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["status"], "pending");
            println!("{} ist beigetreten (pending)", participant);
        }

        // 3. Pending-Liste prüfen – jetzt PendingEntry statt String
        let (status, body) = get_json(&app, &format!("/pending/{}/alice", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        let pending = body.as_array().unwrap();
        assert_eq!(pending.len(), 2);
        println!("Pending-Liste: {} Einträge", pending.len());

        // 4. Beide Teilnehmer genehmigen
        for participant in ["bob", "carol"] {
            let (status, _) = post_json(
                &app,
                "/approve",
                json!({
                    "session_id": session_id,
                    "creator_id": "alice",
                    "participant_id": participant,
                    "approved": true
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            println!("{} genehmigt", participant);
        }

        // 5. Pending-Liste leer
        let (status, body) = get_json(&app, &format!("/pending/{}/alice", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 0);
        println!("Pending-Liste ist leer");

        // 6. Stimmen abgeben
        let bob_vote = encrypt_bool(true, &client_key);
        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "bob",
                "encrypted_votes": [bob_vote]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        println!("Bob hat abgestimmt (Ja)");

        let carol_vote = encrypt_bool(false, &client_key);
        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "carol",
                "encrypted_votes": [carol_vote]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        println!("Carol hat abgestimmt (Nein)");

        // 7. Ergebnisse abrufen
        let (status, body) = get_json(&app, &format!("/results/{}/alice", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ready"], true);

        let results = body["encrypted_results"].as_array().unwrap();
        assert_eq!(results.len(), 1);

        // 8. Ergebnis entschlüsseln
        let result_b64 = results[0].as_str().unwrap();
        let ja_stimmen = decrypt_uint8(result_b64, &client_key);
        assert_eq!(ja_stimmen, 1, "Erwarte 1 Ja-Stimme (bob)");
        println!("Ergebnis: {} von 2 haben Ja gestimmt", ja_stimmen);
    }

    // =========================================================================
    // TEST 2: Single-Choice-Voting-Flow
    // =========================================================================
    // TODO: vote type mismatch (FheBool vs FheUint8 aggregate) — needs rework
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn test_single_choice_voting_full_flow() {
        let (app, _state) = build_app();
        let (client_key, sk_b64) = generate_fhe_keys();

        // 1. Session mit Single-Frage erstellen
        let (status, body) = post_json(
            &app,
            "/session",
            json!({
                "creator_id": "alice",
                "server_key": sk_b64,
                "questions": [{
                    "id": 1,
                    "text": "Welches Framework?",
                    "question_type": "single",
                    "options": ["Axum", "Actix", "Warp"],
                    "multiple": null
                }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let session_id = body["session_id"].as_str().unwrap().to_string();
        println!("Session erstellt: {}", session_id);

        // 2. Teilnehmer beitreten + genehmigen
        for participant in ["bob", "carol", "dave"] {
            post_json(
                &app,
                "/join",
                json!({
                    "session_id": session_id,
                    "participant_id": participant,
                    "enc_name_chunks": null
                }),
            )
            .await;
            post_json(
                &app,
                "/approve",
                json!({
                    "session_id": session_id,
                    "creator_id": "alice",
                    "participant_id": participant,
                    "approved": true
                }),
            )
            .await;
        }
        println!("Alle Teilnehmer genehmigt");

        // 3. Stimmen: bob=0(Axum), carol=0(Axum), dave=1(Actix)
        for (participant, option) in [("bob", 0u8), ("carol", 0u8), ("dave", 1u8)] {
            let encrypted = encrypt_uint8(option, &client_key);
            let (status, _) = post_json(
                &app,
                "/vote",
                json!({
                    "session_id": session_id,
                    "participant_id": participant,
                    "encrypted_votes": [encrypted]
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
            println!("{} hat Option {} gewählt", participant, option);
        }

        // 4. Ergebnisse abrufen
        let (status, body) = get_json(&app, &format!("/results/{}/alice", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ready"], true);

        let results = body["encrypted_results"].as_array().unwrap();
        assert_eq!(results.len(), 1);

        // Summe aller Stimmen (0+0+1=1)
        let result_b64 = results[0].as_str().unwrap();
        let summe = decrypt_uint8(result_b64, &client_key);
        assert_eq!(summe, 1, "Summe sollte 1 sein (0+0+1)");
        println!("Ergebnis Summe: {}", summe);
    }

    // =========================================================================
    // TEST 3: Fehlerfälle
    // =========================================================================
    // TODO: vote type mismatch (FheBool vs FheUint8 aggregate) — needs rework
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore]
    async fn test_error_cases() {
        let (app, _state) = build_app();
        let (client_key, sk_b64) = generate_fhe_keys();

        let (_, body) = post_json(
            &app,
            "/session",
            json!({
                "creator_id": "alice",
                "server_key": sk_b64,
                "questions": [{
                    "id": 1, "text": "Test?", "question_type": "bool",
                    "options": null, "multiple": null
                }]
            }),
        )
        .await;
        let session_id = body["session_id"].as_str().unwrap().to_string();

        // Ungültige Session-ID
        let (status, err_body) = get_json(&app, "/pending/ungueltige-id/alice").await;
        assert!(
            status == StatusCode::INTERNAL_SERVER_ERROR
                || err_body
                    .as_str()
                    .map(|s| s.contains("nicht gefunden"))
                    .unwrap_or(false),
            "Erwarte Fehler für ungültige Session-ID, got status={} body={}",
            status,
            err_body
        );
        println!("Ungültige Session-ID wird abgelehnt");

        // Falscher Creator
        let (status, err_body) = get_json(&app, &format!("/pending/{}/eve", session_id)).await;
        assert!(
            status == StatusCode::INTERNAL_SERVER_ERROR
                || err_body
                    .as_str()
                    .map(|s| s.contains("autorisiert"))
                    .unwrap_or(false),
            "Erwarte Fehler für falschen Creator, got status={} body={}",
            status,
            err_body
        );
        println!("Falscher Creator wird abgelehnt");

        // Nicht genehmigter Teilnehmer
        post_json(
            &app,
            "/join",
            json!({
                "session_id": session_id,
                "participant_id": "mallory",
                "enc_name_chunks": null
            }),
        )
        .await;

        let vote = encrypt_bool(true, &client_key);
        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "mallory",
                "encrypted_votes": [vote]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("Nicht genehmigter Teilnehmer kann nicht abstimmen");

        // Falsche Stimmenanzahl
        post_json(
            &app,
            "/approve",
            json!({
                "session_id": session_id,
                "creator_id": "alice",
                "participant_id": "mallory",
                "approved": true
            }),
        )
        .await;

        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "mallory",
                "encrypted_votes": []
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("Falsche Stimmenanzahl wird abgelehnt");
    }

    // =========================================================================
    // TEST 4: Ergebnisse not ready
    // =========================================================================
    #[tokio::test]
    async fn test_results_not_ready() {
        let (app, _state) = build_app();
        let (_client_key, sk_b64) = generate_fhe_keys();

        let (_, body) = post_json(
            &app,
            "/session",
            json!({
                "creator_id": "alice",
                "server_key": sk_b64,
                "questions": [{
                    "id": 1, "text": "Test?", "question_type": "bool",
                    "options": null, "multiple": null
                }]
            }),
        )
        .await;
        let session_id = body["session_id"].as_str().unwrap().to_string();

        post_json(
            &app,
            "/join",
            json!({
                "session_id": session_id,
                "participant_id": "bob",
                "enc_name_chunks": null
            }),
        )
        .await;
        post_json(
            &app,
            "/approve",
            json!({
                "session_id": session_id,
                "creator_id": "alice",
                "participant_id": "bob",
                "approved": true
            }),
        )
        .await;

        let (status, body) = get_json(&app, &format!("/results/{}/alice", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ready"], false);
        println!("Ergebnisse sind noch nicht bereit");
    }
}
