#[cfg(test)]
mod integration_tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        routing::{get, post},
        Router,
    };
    use base64::{engine::general_purpose, Engine as _};
    use serde_json::{json, Value};
    use tfhe::{generate_keys, prelude::*, CompressedServerKey, ConfigBuilder, FheUint8};
    use tower::util::ServiceExt;

    use encrypted_voting_polling::voting::logic::{
        approve_participant, create_session, finalize_session, get_participants, get_results,
        get_session, get_status, join_session, submit_vote,
    };
    use encrypted_voting_polling::voting::types::AppState;

    // ── Gecachter ServerKey – wird nur einmal generiert ──────────────────────
    static SERVER_KEY_B64: OnceLock<String> = OnceLock::new();

    fn get_server_key_b64() -> String {
        SERVER_KEY_B64
            .get_or_init(|| {
                let config = ConfigBuilder::default().build();
                let (client_key, _) = generate_keys(config);
                let compressed = CompressedServerKey::new(&client_key);
                let sk_bytes = bincode::serialize(&compressed).unwrap();
                general_purpose::STANDARD.encode(&sk_bytes)
            })
            .clone()
    }

    // ── Hilfsfunktion: Router aufbauen ───────────────────────────────────────
    fn build_app() -> (Router, AppState) {
        let state: AppState = Arc::new(Mutex::new(HashMap::new()));
        let app = Router::new()
            .route("/session", post(create_session))
            .route("/join", post(join_session))
            .route(
                "/participants/{session_id}/{creator_id}",
                get(get_participants),
            )
            .route("/approve", post(approve_participant))
            .route("/vote", post(submit_vote))
            .route("/results/{session_id}/{creator_id}", get(get_results))
            .route("/status/{session_id}/{participant_id}", get(get_status))
            .route("/session/{session_id}", get(get_session))
            .route("/finalize/{session_id}/{creator_id}", get(finalize_session))
            .with_state(state.clone())
            .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));
        (app, state)
    }

    // ── Hilfsfunktionen: HTTP-Requests ───────────────────────────────────────
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

    // ── Hilfsfunktionen: FHE ─────────────────────────────────────────────────
    fn generate_fhe_keys() -> (tfhe::ClientKey, String) {
        let config = ConfigBuilder::default().build();
        let (client_key, _) = generate_keys(config);
        let compressed = CompressedServerKey::new(&client_key);
        let sk_bytes = bincode::serialize(&compressed).unwrap();
        let sk_b64 = general_purpose::STANDARD.encode(&sk_bytes);
        (client_key, sk_b64)
    }

    fn encrypt_bool_as_uint8(value: bool, client_key: &tfhe::ClientKey) -> String {
        let val: u8 = if value { 1 } else { 0 };
        let encrypted = FheUint8::encrypt(val, client_key);
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

    // ── Session erstellen (Hilfsfunktion) ────────────────────────────────────
    async fn create_test_session(app: &Router, sk_b64: &str) -> String {
        let (_, body) = post_json(
            app,
            "/session",
            json!({
                "creator_id": "alice",
                "server_key": sk_b64,
                "questions": [{
                    "id": 1, "text": "Test?", "question_type": "bool",
                    "options": null,
                }]
            }),
        )
        .await;
        body["session_id"].as_str().unwrap().to_string()
    }

    // =========================================================================
    // TEST 1: Single-Choice-Voting-Flow (braucht FHE-Berechnung)
    // =========================================================================
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_single_choice_voting_full_flow() {
        let (app, _) = build_app();
        let (client_key, sk_b64) = generate_fhe_keys();

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
                }]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let session_id = body["session_id"].as_str().unwrap().to_string();
        println!("✅ Session erstellt: {}", session_id);

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
        println!("✅ Alle Teilnehmer genehmigt");

        for (participant, votes) in [
            ("bob", vec![1u8, 0u8, 0u8]),
            ("carol", vec![1u8, 0u8, 0u8]),
            ("dave", vec![0u8, 1u8, 0u8]),
        ] {
            let enc_votes: Vec<String> = votes
                .iter()
                .map(|&v| encrypt_uint8(v, &client_key))
                .collect();
            let (status, _) = post_json(
                &app,
                "/vote",
                json!({
                    "session_id": session_id,
                    "participant_id": participant,
                    "encrypted_votes": [enc_votes]
                }),
            )
            .await;
            assert_eq!(status, StatusCode::OK);
        }
        println!("✅ Stimmen abgegeben");

        let (status, body) = get_json(&app, &format!("/results/{}/alice", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ready"], true);

        let results = body["encrypted_results"].as_array().unwrap();
        let option_results = results[0].as_array().unwrap();

        let axum_votes = decrypt_uint8(option_results[0].as_str().unwrap(), &client_key);
        let actix_votes = decrypt_uint8(option_results[1].as_str().unwrap(), &client_key);
        let warp_votes = decrypt_uint8(option_results[2].as_str().unwrap(), &client_key);

        assert_eq!(axum_votes, 2);
        assert_eq!(actix_votes, 1);
        assert_eq!(warp_votes, 0);
        println!(
            "✅ Axum={}, Actix={}, Warp={}",
            axum_votes, actix_votes, warp_votes
        );
    }

    // =========================================================================
    // TEST 2: Fehlerfälle
    // =========================================================================
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_error_cases() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();
        let (client_key, _) = generate_fhe_keys();

        let session_id = create_test_session(&app, &sk_b64).await;

        // Ungültige Session-ID
        let (status, err_body) = get_json(&app, "/participants/ungueltige-id/alice").await;
        assert!(
            status == StatusCode::INTERNAL_SERVER_ERROR
                || err_body
                    .as_str()
                    .map(|s| s.contains("nicht gefunden"))
                    .unwrap_or(false)
        );
        println!("✅ Ungültige Session-ID wird abgelehnt");

        // Falscher Creator bei pending
        let (status, err_body) = get_json(&app, &format!("/participants/{}/eve", session_id)).await;
        assert!(
            status == StatusCode::INTERNAL_SERVER_ERROR
                || err_body
                    .as_str()
                    .map(|s| s.contains("autorisiert"))
                    .unwrap_or(false)
        );
        println!("✅ Falscher Creator bei pending wird abgelehnt");

        // Falscher Creator bei results
        let (status, _) = get_json(&app, &format!("/results/{}/eve", session_id)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Falscher Creator bei results wird abgelehnt");

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

        let vote = encrypt_bool_as_uint8(true, &client_key);
        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "mallory",
                "encrypted_votes": [[vote]]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Nicht genehmigter Teilnehmer kann nicht abstimmen");

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
        println!("✅ Falsche Stimmenanzahl wird abgelehnt");
    }

    // =========================================================================
    // TEST 3: Ergebnisse not ready (kein FHE nötig)
    // =========================================================================
    #[tokio::test]
    async fn test_results_not_ready() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

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
        println!("✅ Ergebnisse sind noch nicht bereit");
    }

    // =========================================================================
    // TEST 4: finalize_session (kein FHE nötig)
    // =========================================================================
    #[tokio::test]
    async fn test_finalize_session() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        // Ungültige Session
        let (status, _) = get_json(&app, "/finalize/ungueltig/alice").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Ungültige Session gibt Fehler");

        // Falscher Creator
        let (status, _) = get_json(&app, &format!("/finalize/{}/eve", session_id)).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Falscher Creator wird abgelehnt");

        // Korrekt finalisieren
        let (status, body) = get_json(&app, &format!("/finalize/{}/alice", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "finalized");
        println!("✅ Session erfolgreich finalisiert");
    }

    // =========================================================================
    // TEST 5: get_status (kein FHE nötig)
    // =========================================================================
    #[tokio::test]
    async fn test_get_status() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        // Ungültige Session
        let (status, _) = get_json(&app, "/status/ungueltig/bob").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        println!("✅ Ungültige Session gibt NOT_FOUND");

        // Teilnehmer nicht in Session
        let (status, body) = get_json(&app, &format!("/status/{}/bob", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "not_found");
        println!("✅ Unbekannter Teilnehmer gibt not_found");

        // Teilnehmer beitreten → pending
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
        let (status, body) = get_json(&app, &format!("/status/{}/bob", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "pending");
        println!("✅ Teilnehmer ist pending");

        // Teilnehmer genehmigen → approved
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
        let (status, body) = get_json(&app, &format!("/status/{}/bob", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "approved");
        println!("✅ Teilnehmer ist approved");
    }

    // =========================================================================
    // TEST 6: get_session (kein FHE nötig)
    // =========================================================================
    #[tokio::test]
    async fn test_get_session() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        // Ungültige Session
        let (status, _) = get_json(&app, "/session/ungueltig").await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Ungültige Session gibt Fehler");

        let session_id = create_test_session(&app, &sk_b64).await;

        let (status, body) = get_json(&app, &format!("/session/{}", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["session_id"], session_id);
        assert_eq!(body["questions"].as_array().unwrap().len(), 1);
        println!("✅ Session erfolgreich abgerufen");
    }

    // =========================================================================
    // TEST 7: create_session Fehlerfälle (kein FHE nötig)
    // =========================================================================
    #[tokio::test]
    async fn test_create_session_errors() {
        let (app, _) = build_app();

        // Ungültiger Base64
        let (status, _) = post_json(
            &app,
            "/session",
            json!({
                "creator_id": "alice",
                "server_key": "!!!ungültiger-base64!!!",
                "questions": []
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Ungültiger Base64 wird abgelehnt");

        // Gültiger Base64 aber kein gültiger ServerKey
        let (status, _) = post_json(
            &app,
            "/session",
            json!({
                "creator_id": "alice",
                "server_key": "dGVzdA==",
                "questions": []
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Ungültiger ServerKey wird abgelehnt");
    }

    // =========================================================================
    // TEST 8: join_session Fehlerfälle (kein FHE nötig)
    // =========================================================================
    #[tokio::test]
    async fn test_join_session_errors() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        // Ungültige Session-ID beim Join
        let (status, _) = post_json(
            &app,
            "/join",
            json!({
                "session_id": "ungueltig",
                "participant_id": "bob",
                "enc_name_chunks": null
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Ungültige Session-ID beim Join wird abgelehnt");

        let session_id = create_test_session(&app, &sk_b64).await;

        // Session finalisieren
        get_json(&app, &format!("/finalize/{}/alice", session_id)).await;

        // Join nach Finalisierung
        let (status, _) = post_json(
            &app,
            "/join",
            json!({
                "session_id": session_id,
                "participant_id": "bob",
                "enc_name_chunks": null
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Join nach Finalisierung wird abgelehnt");
    }

    // =========================================================================
    // TEST 9: approve_participant Fehlerfälle + Ablehnen
    // =========================================================================
    #[tokio::test]
    async fn test_approve_participant_cases() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        // Teilnehmer beitreten
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

        // Ungültige Session-ID bei approve
        let (status, _) = post_json(
            &app,
            "/approve",
            json!({
                "session_id": "ungueltig",
                "creator_id": "alice",
                "participant_id": "bob",
                "approved": true
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Ungültige Session-ID bei approve wird abgelehnt");

        // Falscher Creator bei approve
        let (status, _) = post_json(
            &app,
            "/approve",
            json!({
                "session_id": session_id,
                "creator_id": "eve",
                "participant_id": "bob",
                "approved": true
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Falscher Creator bei approve wird abgelehnt");

        // Teilnehmer ablehnen (approved: false)
        let (status, body) = post_json(
            &app,
            "/approve",
            json!({
                "session_id": session_id,
                "creator_id": "alice",
                "participant_id": "bob",
                "approved": false
            }),
        )
        .await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ok");
        println!("✅ Teilnehmer erfolgreich abgelehnt");

        // Teilnehmerliste abrufen
        let (status, body) = get_json(&app, &format!("/participants/{}/alice", session_id)).await;

        assert_eq!(status, StatusCode::OK);

        let participants = body.as_array().unwrap();

        let bob = participants.iter().find(|p| p["participant_id"] == "bob");

        assert!(bob.is_none(), "bob sollte nach Ablehnung entfernt sein");
    }

    // =========================================================================
    // TEST 10: vote nach Finalisierung
    // =========================================================================
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_vote_after_finalization() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();
        let (client_key, _) = generate_fhe_keys();

        let session_id = create_test_session(&app, &sk_b64).await;

        // Teilnehmer beitreten + genehmigen
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

        // Session finalisieren
        get_json(&app, &format!("/finalize/{}/alice", session_id)).await;

        // Vote nach Finalisierung → Fehler
        let vote = encrypt_bool_as_uint8(true, &client_key);
        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "bob",
                "encrypted_votes": [[vote]]
            }),
        )
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Vote nach Finalisierung wird abgelehnt");
    }

    #[tokio::test]
    async fn test_double_join_same_participant() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

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

        // second join (should overwrite or be ignored depending on design)
        let (status, _) = post_json(
            &app,
            "/join",
            json!({
                "session_id": session_id,
                "participant_id": "bob",
                "enc_name_chunks": null
            }),
        )
        .await;

        assert_eq!(status, StatusCode::OK);

        let (_, participants) =
            get_json(&app, &format!("/participants/{}/alice", session_id)).await;

        let list = participants.as_array().unwrap();
        let bob_count = list.iter().filter(|p| p["participant_id"] == "bob").count();

        assert_eq!(bob_count, 1);
    }

    #[tokio::test]
    async fn test_vote_without_join() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();
        let (client_key, _) = generate_fhe_keys();

        let session_id = create_test_session(&app, &sk_b64).await;

        let vote = encrypt_uint8(1, &client_key);

        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "ghost",
                "encrypted_votes": [[vote]]
            }),
        )
        .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_approve_after_finalize() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        get_json(&app, &format!("/finalize/{}/alice", session_id)).await;

        let (status, _) = post_json(
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

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_join_after_votes_exist() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();
        let (client_key, _) = generate_fhe_keys();

        let session_id = create_test_session(&app, &sk_b64).await;

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

        let vote = encrypt_uint8(1, &client_key);

        post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "bob",
                "encrypted_votes": [[vote]]
            }),
        )
        .await;

        // neuer participant nach vote
        let (status, _) = post_json(
            &app,
            "/join",
            json!({
                "session_id": session_id,
                "participant_id": "carol",
                "enc_name_chunks": null
            }),
        )
        .await;

        // je nach design: entweder erlaubt oder verboten
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn test_results_with_no_votes() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

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
    }

    #[tokio::test]
    async fn test_status_spoofing() {
        let (app, _) = build_app();
        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        let (status, body) = get_json(&app, &format!("/status/{}/alice", session_id)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "not_found");
    }

    #[tokio::test]
    async fn test_vote_empty_session_questions() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

        let (_, body) = post_json(
            &app,
            "/session",
            json!({
                "creator_id": "alice",
                "server_key": sk_b64,
                "questions": []
            }),
        )
        .await;

        let session_id = body["session_id"].as_str().unwrap();

        let (client_key, _) = generate_fhe_keys();
        let vote = encrypt_uint8(1, &client_key);

        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "bob",
                "encrypted_votes": []
            }),
        )
        .await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }
}
