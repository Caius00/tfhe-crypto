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

    use tfhe::{
        generate_keys, prelude::*, CompactCiphertextList, CompactPublicKey, CompressedServerKey,
        ConfigBuilder, FheUint32,
    };

    use tower::util::ServiceExt;

    use encrypted_voting_polling::voting::logic::{
        approve_participant, create_session, finalize_session, get_participants, get_results,
        get_session, get_status, join_session, submit_vote,
    };

    use encrypted_voting_polling::voting::types::AppState;

    // ── Gecachter ServerKey ───────────────────────────────────────────────────

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

    // Vereinfachte Version ohne CompactPublicKey

    fn generate_fhe_keys() -> (tfhe::ClientKey, String) {
        let config = ConfigBuilder::default().build();

        let (client_key, _) = generate_keys(config);

        let compressed = CompressedServerKey::new(&client_key);

        let sk_bytes = bincode::serialize(&compressed).unwrap();

        let sk_b64 = general_purpose::STANDARD.encode(&sk_bytes);

        (client_key, sk_b64)
    }

    // Wie Frontend: FheUint32 mit CompactPublicKey (One-Hot/Multi-Hot)

    fn encrypt_uint32_with_public(value: u32, client_key: &tfhe::ClientKey) -> String {
        let encrypted = FheUint32::encrypt(value, client_key);

        let bytes = bincode::serialize(&encrypted).unwrap();

        general_purpose::STANDARD.encode(&bytes)
    }

    fn decrypt_uint32(b64: &str, client_key: &tfhe::ClientKey) -> u32 {
        let bytes = general_purpose::STANDARD.decode(b64).unwrap();

        let encrypted: FheUint32 = bincode::deserialize(&bytes).unwrap();

        encrypted.decrypt(client_key)
    }

    // Fake-Vote für Fehlerfälle (kein echter FHE-Wert nötig)

    fn fake_vote() -> String {
        general_purpose::STANDARD.encode(vec![0u8; 32])
    }

    async fn create_test_session(app: &Router, sk_b64: &str) -> String {
        let (_, body) = post_json(
            app,
            "/session",
            json!({

                "creator_id": "alice",

                "server_key": sk_b64,

                "questions": [{

                    "id": 1, "text": "Test?", "question_type": "single",

                    "options": ["Ja", "Nein"], "multiple": null

                }]

            }),
        )
        .await;

        body["session_id"].as_str().unwrap().to_string()
    }

    // =========================================================================

    // TEST 1: Single-Choice-Voting-Flow mit FheUint32 + CompactPublicKey

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

                    "multiple": null

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

        // One-Hot Encoding wie Frontend:

        // bob   → Axum  → [1, 0, 0]

        // carol → Axum  → [1, 0, 0]

        // dave  → Actix → [0, 1, 0]

        for (participant, votes) in [
            ("bob", vec![1u32, 0u32, 0u32]),
            ("carol", vec![1u32, 0u32, 0u32]),
            ("dave", vec![0u32, 1u32, 0u32]),
        ] {
            let enc_votes: Vec<String> = votes
                .iter()
                .map(|&v| encrypt_uint32_with_public(v, &client_key))
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

        println!("✅ Stimmen abgegeben (FheUint32 + CompactPublicKey)");

        let (status, body) = get_json(&app, &format!("/results/{}/alice", session_id)).await;

        assert_eq!(status, StatusCode::OK);

        assert_eq!(body["ready"], true);

        let results = body["encrypted_results"].as_array().unwrap();

        let option_results = results[0].as_array().unwrap();

        let axum_votes = decrypt_uint32(option_results[0].as_str().unwrap(), &client_key);

        let actix_votes = decrypt_uint32(option_results[1].as_str().unwrap(), &client_key);

        let warp_votes = decrypt_uint32(option_results[2].as_str().unwrap(), &client_key);

        assert_eq!(axum_votes, 2);

        assert_eq!(actix_votes, 1);

        assert_eq!(warp_votes, 0);

        println!(
            "✅ Axum={}, Actix={}, Warp={}",
            axum_votes, actix_votes, warp_votes
        );
    }

    // =========================================================================

    // TEST 2: Numeric-Voting-Flow mit FheUint32

    // =========================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]

    async fn test_numeric_voting_full_flow() {
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

                    "text": "Wie viele Punkte?",

                    "question_type": "numeric",

                    "options": null,

                    "multiple": null

                }]

            }),
        )
        .await;

        assert_eq!(status, StatusCode::OK);

        let session_id = body["session_id"].as_str().unwrap().to_string();

        for participant in ["bob", "carol"] {
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

        // bob → 42, carol → 58 → Summe = 100

        for (participant, value) in [("bob", 42u32), ("carol", 58u32)] {
            let enc = encrypt_uint32_with_public(value, &client_key);

            let (status, _) = post_json(
                &app,
                "/vote",
                json!({

                    "session_id": session_id,

                    "participant_id": participant,

                    "encrypted_votes": [[enc]]

                }),
            )
            .await;

            assert_eq!(status, StatusCode::OK);
        }

        println!("✅ Numerische Stimmen abgegeben");

        let (status, body) = get_json(&app, &format!("/results/{}/alice", session_id)).await;

        assert_eq!(status, StatusCode::OK);

        assert_eq!(body["ready"], true);

        let results = body["encrypted_results"].as_array().unwrap();

        let result_b64 = results[0].as_array().unwrap()[0].as_str().unwrap();

        let summe = decrypt_uint32(result_b64, &client_key);

        assert_eq!(summe, 100);

        println!("✅ Summe: {} (42 + 58)", summe);
    }

    // =========================================================================

    // TEST 3: Fehlerfälle (Fake-Votes statt echter FHE)

    // =========================================================================

    #[tokio::test]

    async fn test_error_cases() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        // Ungültige Session-ID

        let (status, err_body) = get_json(&app, "/participants/ungueltige-id/alice").await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        println!("✅ Ungültige Session-ID wird abgelehnt");

        // Falscher Creator bei pending

        let (status, err_body) = get_json(&app, &format!("/participants/{}/eve", session_id)).await;

        assert_eq!(status, StatusCode::FORBIDDEN);

        println!("✅ Falscher Creator bei pending wird abgelehnt");

        // Falscher Creator bei results

        let (status, _) = get_json(&app, &format!("/results/{}/eve", session_id)).await;

        assert_eq!(status, StatusCode::FORBIDDEN);

        println!("✅ Falscher Creator bei results wird abgelehnt");

        // Teilnehmer existiert nicht

        let (status, _) = post_json(
            &app,
            "/vote",
            json!({
                "session_id": session_id,
                "participant_id": "does-not-exist",
                "encrypted_votes": [[fake_vote()]]
            }),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        println!("✅ Unbekannter Teilnehmer wird abgelehnt");

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

        let (status, _) = post_json(
            &app,
            "/vote",
            json!({

                "session_id": session_id,

                "participant_id": "mallory",

                "encrypted_votes": [[fake_vote()]]

            }),
        )
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);

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

        assert_eq!(status, StatusCode::BAD_REQUEST);

        println!("✅ Falsche Stimmenanzahl wird abgelehnt");
    }

    // =========================================================================

    // TEST 4: Ergebnisse not ready

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

    #[tokio::test]
    async fn test_get_results_errors() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        // Session existiert nicht

        let (status, _) = get_json(&app, "/results/ungueltig/alice").await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        println!("✅ Ungültige Session bei results wird abgelehnt");

        // Falscher Creator

        let (status, _) = get_json(&app, &format!("/results/{}/eve", session_id)).await;

        assert_eq!(status, StatusCode::FORBIDDEN);

        println!("✅ Falscher Creator bei results wird abgelehnt");
    }

    // =========================================================================

    // TEST 5: finalize_session

    // =========================================================================

    #[tokio::test]

    async fn test_finalize_session() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        let (status, _) = get_json(&app, "/finalize/ungueltig/alice").await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        println!("✅ Ungültige Session gibt Fehler");

        let (status, _) = get_json(&app, &format!("/finalize/{}/eve", session_id)).await;

        assert_eq!(status, StatusCode::FORBIDDEN);

        println!("✅ Falscher Creator wird abgelehnt");

        let (status, body) = get_json(&app, &format!("/finalize/{}/alice", session_id)).await;

        assert_eq!(status, StatusCode::OK);

        assert_eq!(body["status"], "finalized");

        println!("✅ Session erfolgreich finalisiert");

        //  erneutes Finalisieren → CONFLICT
        let (status, _) = get_json(&app, &format!("/finalize/{}/alice", session_id)).await;
        assert_eq!(status, StatusCode::CONFLICT);

        println!("✅ Doppelte Finalisierung wird abgelehnt");
    }

    // =========================================================================

    // TEST 6: get_status

    // =========================================================================

    #[tokio::test]

    async fn test_get_status() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        let (status, _) = get_json(&app, "/status/ungueltig/bob").await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        let (status, body) = get_json(&app, &format!("/status/{}/bob", session_id)).await;

        assert_eq!(status, StatusCode::OK);

        assert_eq!(body["status"], "not_found");

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

        // anderer Teilnehmer existiert nicht
        let (status, body) = get_json(&app, &format!("/status/{}/mallory", session_id)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "not_found");

        println!("✅ Status-Flow korrekt");
    }

    // =========================================================================

    // TEST 7: get_session

    // =========================================================================

    #[tokio::test]

    async fn test_get_session() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

        let (status, _) = get_json(&app, "/session/ungueltig").await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        let session_id = create_test_session(&app, &sk_b64).await;

        let (status, body) = get_json(&app, &format!("/session/{}", session_id)).await;

        assert_eq!(status, StatusCode::OK);

        assert_eq!(body["session_id"], session_id);

        assert_eq!(body["questions"].as_array().unwrap().len(), 1);

        assert!(body.get("public_key").is_some());

        println!("✅ Session erfolgreich abgerufen");
    }

    // =========================================================================

    // TEST 8: create_session Fehlerfälle

    // =========================================================================

    #[tokio::test]

    async fn test_create_session_errors() {
        let (app, _) = build_app();

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

        assert_eq!(status, StatusCode::BAD_REQUEST);

        println!("✅ Ungültiger Base64 wird abgelehnt");

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

        assert_eq!(status, StatusCode::BAD_REQUEST);

        println!("✅ Ungültiger ServerKey wird abgelehnt");
    }

    // =========================================================================

    // TEST 9: join_session Fehlerfälle

    // =========================================================================

    #[tokio::test]

    async fn test_join_session_errors() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

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

        assert_eq!(status, StatusCode::NOT_FOUND);

        println!("✅ Ungültige Session-ID beim Join wird abgelehnt");

        let session_id = create_test_session(&app, &sk_b64).await;

        get_json(&app, &format!("/finalize/{}/alice", session_id)).await;

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

        assert_eq!(status, StatusCode::CONFLICT);

        println!("✅ Join nach Finalisierung wird abgelehnt");
    }

    // =========================================================================

    // TEST 10: approve_participant Fehlerfälle

    // =========================================================================

    #[tokio::test]
    async fn test_approve_participant_cases() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        // ✔️ Join
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

        // ❌ 1. falsche Session
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

        assert_eq!(status, StatusCode::NOT_FOUND);

        println!("✅ Ungültige Session-ID bei approve wird abgelehnt");

        // ❌ 2. falscher Creator
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

        assert_eq!(status, StatusCode::FORBIDDEN);

        println!("✅ Falscher Creator wird abgelehnt");

        // ❌ 3. Teilnehmer existiert nicht
        let (status, _) = post_json(
            &app,
            "/approve",
            json!({
                "session_id": session_id,
                "creator_id": "alice",
                "participant_id": "does-not-exist",
                "approved": true
            }),
        )
        .await;

        assert_eq!(status, StatusCode::NOT_FOUND);

        println!("✅ Unbekannter Teilnehmer wird abgelehnt");

        // ✔️ Ablehnung von bob
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

        // ✔️ Teilnehmerliste leer
        let (status, body) = get_json(&app, &format!("/participants/{}/alice", session_id)).await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 0);

        println!("✅ Pending-Liste leer nach Ablehnung");

        // ✔️ Session finalisieren korrekt testen
        let (status, _) = get_json(&app, &format!("/finalize/{}/alice", session_id)).await;

        assert_eq!(status, StatusCode::OK);

        // ❌ 4. approve nach finalization → CONFLICT
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

        assert_eq!(status, StatusCode::CONFLICT);

        println!("✅ Approve nach Finalisierung wird abgelehnt");
    }

    // =========================================================================

    // TEST 11: vote nach Finalisierung

    // =========================================================================

    #[tokio::test]

    async fn test_vote_after_finalization() {
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

        get_json(&app, &format!("/finalize/{}/alice", session_id)).await;

        let (status, _) = post_json(
            &app,
            "/vote",
            json!({

                "session_id": session_id,

                "participant_id": "bob",

                "encrypted_votes": [[fake_vote()]]

            }),
        )
        .await;

        assert_eq!(status, StatusCode::CONFLICT);

        println!("✅ Vote nach Finalisierung wird abgelehnt");
    }

    // =========================================================================

    // TEST 12: get_participants mit enc_name_chunks

    // =========================================================================

    #[tokio::test]

    async fn test_participants_with_enc_name_chunks() {
        let (app, _) = build_app();

        let sk_b64 = get_server_key_b64();

        let session_id = create_test_session(&app, &sk_b64).await;

        // Teilnehmer mit verschlüsseltem Namen beitreten lassen

        post_json(
            &app,
            "/join",
            json!({

                "session_id": session_id,

                "participant_id": "bob",

                "enc_name_chunks": ["aGVsbG8=", "d29ybGQ="]

            }),
        )
        .await;

        let (status, body) = get_json(&app, &format!("/participants/{}/alice", session_id)).await;

        assert_eq!(status, StatusCode::OK);

        let pending = body.as_array().unwrap();

        assert_eq!(pending.len(), 1);

        assert_eq!(pending[0]["participant_id"], "bob");

        assert!(pending[0]["enc_name_chunks"].is_array());

        println!("✅ Pending-Eintrag mit enc_name_chunks korrekt");
    }
}
