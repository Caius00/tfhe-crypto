#[cfg(test)]
mod integration_tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use axum::{
        body::Body,
        http::{Request, StatusCode},
        Router,
        routing::{get, post},
    };
    use base64::{engine::general_purpose, Engine as _};
    use serde_json::{json, Value};
    use tower::util::ServiceExt; // für `.oneshot()`
    use tfhe::{
        generate_keys, set_server_key, ConfigBuilder,
        prelude::*,
        CompressedServerKey, FheBool, FheUint8,
    };

    use encrypted_voting_polling::voting::types::AppState;
    use encrypted_voting_polling::voting::logic::{
        create_session, join_session, get_pending,
        approve_participant, submit_vote, get_results,
    };

    // ── Hilfsfunktion: Router mit frischem State aufbauen ────────────────────
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

    // ── Hilfsfunktion: JSON-Request senden ───────────────────────────────────
    async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
        let req = Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
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
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        (status, json)
    }

    // ── Hilfsfunktion: FHE-Keys generieren und als Base64 zurückgeben ────────
    fn generate_fhe_keys() -> (tfhe::ClientKey, String) {
        let config = ConfigBuilder::default().build();
        let (client_key, server_key) = generate_keys(config);
        let compressed = CompressedServerKey::new(&client_key);
        let sk_bytes = bincode::serialize(&compressed).unwrap();
        let sk_b64 = general_purpose::STANDARD.encode(&sk_bytes);
        (client_key, sk_b64)
    }

    // ── Hilfsfunktion: FheBool verschlüsseln → Base64 ────────────────────────
    fn encrypt_bool(value: bool, client_key: &tfhe::ClientKey) -> String {
        let encrypted = FheBool::encrypt(value, client_key);
        let bytes = bincode::serialize(&encrypted).unwrap();
        general_purpose::STANDARD.encode(&bytes)
    }

    // ── Hilfsfunktion: FheUint8 verschlüsseln → Base64 ───────────────────────
    fn encrypt_choice(option_idx: u8, client_key: &tfhe::ClientKey) -> String {
        let encrypted = FheUint8::encrypt(option_idx, client_key);
        let bytes = bincode::serialize(&encrypted).unwrap();
        general_purpose::STANDARD.encode(&bytes)
    }

    // ── Hilfsfunktion: FheUint8 entschlüsseln aus Base64 ─────────────────────
    fn decrypt_uint8(b64: &str, client_key: &tfhe::ClientKey) -> u8 {
        let bytes = general_purpose::STANDARD.decode(b64).unwrap();
        let encrypted: FheUint8 = bincode::deserialize(&bytes).unwrap();
        encrypted.decrypt(client_key)
    }

    // =========================================================================
    // TEST 1: Kompletter Bool-Voting-Flow (Ja/Nein)
    // =========================================================================
    #[tokio::test(flavor = "multi_thread",worker_threads = 2)]
    async fn test_bool_voting_full_flow() {
        let (app, _state) = build_app();
        let (client_key, sk_b64) = generate_fhe_keys();

        // 1. Session erstellen
        let (status, body) = post_json(&app, "/session", json!({
            "creator_id": "alice",
            "server_key": sk_b64,
            "questions": [{
                "id": 1,
                "text": "Soll das Projekt fortgesetzt werden?",
                "question_type": "bool",
                "options": null
            }]
        })).await;
        assert_eq!(status, StatusCode::OK, "Session erstellen fehlgeschlagen");
        let session_id = body["session_id"].as_str().unwrap().to_string();
        println!("✅ Session erstellt: {}", session_id);

        // 2. Zwei Teilnehmer beitreten lassen
        for participant in ["bob", "carol"] {
            let (status, body) = post_json(&app, "/join", json!({
                "session_id": session_id,
                "participant_id": participant
            })).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body["status"], "pending");
            println!("✅ {} ist beigetreten (pending)", participant);
        }

        // 3. Pending-Liste prüfen
        let (status, body) = get_json(
            &app,
            &format!("/pending/{}/alice", session_id)
        ).await;
        assert_eq!(status, StatusCode::OK);
        let pending = body.as_array().unwrap();
        assert_eq!(pending.len(), 2);
        println!("✅ Pending-Liste: {:?}", pending);

        // 4. Beide Teilnehmer genehmigen
        for participant in ["bob", "carol"] {
            let (status, _) = post_json(&app, "/approve", json!({
                "session_id": session_id,
                "creator_id": "alice",
                "participant_id": participant,
                "approved": true
            })).await;
            assert_eq!(status, StatusCode::OK);
            println!("✅ {} genehmigt", participant);
        }

        // 5. Pending-Liste sollte jetzt leer sein
        let (status, body) = get_json(
            &app,
            &format!("/pending/{}/alice", session_id)
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body.as_array().unwrap().len(), 0);
        println!("✅ Pending-Liste ist leer");

        // 6. Stimmen abgeben: bob = Ja, carol = Nein
        let bob_vote = encrypt_bool(true, &client_key);
        let (status, _) = post_json(&app, "/vote", json!({
            "session_id": session_id,
            "participant_id": "bob",
            "encrypted_votes": [bob_vote]
        })).await;
        assert_eq!(status, StatusCode::OK);
        println!("✅ Bob hat abgestimmt (Ja)");

        let carol_vote = encrypt_bool(false, &client_key);
        let (status, _) = post_json(&app, "/vote", json!({
            "session_id": session_id,
            "participant_id": "carol",
            "encrypted_votes": [carol_vote]
        })).await;
        assert_eq!(status, StatusCode::OK);
        println!("✅ Carol hat abgestimmt (Nein)");

        // 7. Ergebnisse abrufen
        let (status, body) = get_json(
            &app,
            &format!("/results/{}/alice", session_id)
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ready"], true);

        let results = body["encrypted_results"].as_array().unwrap();
        assert_eq!(results.len(), 1);

        // 8. Ergebnis entschlüsseln und prüfen
        let result_b64 = results[0].as_str().unwrap();
        let ja_stimmen = decrypt_uint8(result_b64, &client_key);
        assert_eq!(ja_stimmen, 1, "Erwarte 1 Ja-Stimme (bob)");
        println!("✅ Ergebnis: {} von 2 haben Ja gestimmt", ja_stimmen);
    }

    // =========================================================================
    // TEST 2: Multiple-Choice-Voting-Flow
    // =========================================================================
    #[tokio::test(flavor = "multi_thread",worker_threads = 2)]
    async fn test_choice_voting_full_flow() {
        let (app, _state) = build_app();
        let (client_key, sk_b64) = generate_fhe_keys();

        // 1. Session mit Choice-Frage erstellen
        let (status, body) = post_json(&app, "/session", json!({
            "creator_id": "alice",
            "server_key": sk_b64,
            "questions": [{
                "id": 1,
                "text": "Welches Framework sollen wir nutzen?",
                "question_type": "choice",
                "options": ["Axum", "Actix", "Warp"]
            }]
        })).await;
        assert_eq!(status, StatusCode::OK);
        let session_id = body["session_id"].as_str().unwrap().to_string();
        println!("✅ Session erstellt: {}", session_id);

        // 2. Drei Teilnehmer beitreten + genehmigen
        for participant in ["bob", "carol", "dave"] {
            post_json(&app, "/join", json!({
                "session_id": session_id,
                "participant_id": participant
            })).await;
            post_json(&app, "/approve", json!({
                "session_id": session_id,
                "creator_id": "alice",
                "participant_id": participant,
                "approved": true
            })).await;
        }
        println!("✅ Alle Teilnehmer genehmigt");

        // 3. Stimmen abgeben: bob=Axum(0), carol=Axum(0), dave=Actix(1)
        let votes = [("bob", 0u8), ("carol", 0u8), ("dave", 1u8)];
        for (participant, option) in votes {
            let encrypted = encrypt_choice(option, &client_key);
            let (status, _) = post_json(&app, "/vote", json!({
                "session_id": session_id,
                "participant_id": participant,
                "encrypted_votes": [encrypted]
            })).await;
            assert_eq!(status, StatusCode::OK);
            println!("✅ {} hat Option {} gewählt", participant, option);
        }

        // 4. Ergebnisse abrufen
        let (status, body) = get_json(
            &app,
            &format!("/results/{}/alice", session_id)
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ready"], true);

        let results = body["encrypted_results"].as_array().unwrap();
        assert_eq!(results.len(), 1);

        // Das Ergebnis ist ein JSON-Array von Base64-Strings (eine Summe pro Option)
        let option_results: Vec<String> =
            serde_json::from_str(results[0].as_str().unwrap()).unwrap();
        assert_eq!(option_results.len(), 3); // 3 Optionen

        let axum_votes = decrypt_uint8(&option_results[0], &client_key);
        let actix_votes = decrypt_uint8(&option_results[1], &client_key);
        let warp_votes = decrypt_uint8(&option_results[2], &client_key);

        assert_eq!(axum_votes, 2, "Axum sollte 2 Stimmen haben");
        assert_eq!(actix_votes, 1, "Actix sollte 1 Stimme haben");
        assert_eq!(warp_votes, 0, "Warp sollte 0 Stimmen haben");

        println!("✅ Ergebnisse: Axum={}, Actix={}, Warp={}", axum_votes, actix_votes, warp_votes);
    }

    // =========================================================================
    // TEST 3: Fehlerfälle
    // =========================================================================
    #[tokio::test(flavor = "multi_thread",worker_threads = 2)]
    async fn test_error_cases() {
        let (app, _state) = build_app();
        let (client_key, sk_b64) = generate_fhe_keys();

        // Session erstellen
        let (_, body) = post_json(&app, "/session", json!({
            "creator_id": "alice",
            "server_key": sk_b64,
            "questions": [{
                "id": 1, "text": "Test?", "question_type": "bool", "options": null
            }]
        })).await;
        let session_id = body["session_id"].as_str().unwrap().to_string();

        // Ungültige Session-ID
        let (status, err_body) = get_json(&app, "/pending/ungueltige-id/alice").await;  // ← _ zu err_body
        assert!(
            status == StatusCode::INTERNAL_SERVER_ERROR || err_body.as_str().map(|s| s.contains("nicht gefunden")).unwrap_or(false),
            "Erwarte Fehler für ungültige Session-ID, got status={} body={}", status, err_body  // ← body zu err_body
        );
        println!("✅ Ungültige Session-ID wird abgelehnt");

        // Falscher Creator
        let (status, err_body) = get_json(
            &app,
            &format!("/pending/{}/eve", session_id)
        ).await;
        assert!(
            status == StatusCode::INTERNAL_SERVER_ERROR || err_body.as_str().map(|s| s.contains("autorisiert")).unwrap_or(false),
            "Erwarte Fehler für falschen Creator, got status={} body={}", status, err_body
        );
        println!("✅ Falscher Creator wird abgelehnt");

        // Nicht genehmigter Teilnehmer versucht abzustimmen
        post_json(&app, "/join", json!({
            "session_id": session_id,
            "participant_id": "mallory"
        })).await;

        let vote = encrypt_bool(true, &client_key);
        let (status, _) = post_json(&app, "/vote", json!({
            "session_id": session_id,
            "participant_id": "mallory",
            "encrypted_votes": [vote]
        })).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Nicht genehmigter Teilnehmer kann nicht abstimmen");

        // Falsche Anzahl an Stimmen
        post_json(&app, "/approve", json!({
            "session_id": session_id,
            "creator_id": "alice",
            "participant_id": "mallory",
            "approved": true
        })).await;

        let (status, _) = post_json(&app, "/vote", json!({
            "session_id": session_id,
            "participant_id": "mallory",
            "encrypted_votes": [] // leer, aber 1 Frage erwartet
        })).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        println!("✅ Falsche Stimmenanzahl wird abgelehnt");
    }

    // =========================================================================
    // TEST 4: Ergebnisse nicht ready wenn noch nicht alle abgestimmt haben
    // =========================================================================
    #[tokio::test]
    async fn test_results_not_ready() {
        let (app, _state) = build_app();
        let (_client_key, sk_b64) = generate_fhe_keys();

        let (_, body) = post_json(&app, "/session", json!({
            "creator_id": "alice",
            "server_key": sk_b64,
            "questions": [{
                "id": 1, "text": "Test?", "question_type": "bool", "options": null
            }]
        })).await;
        let session_id = body["session_id"].as_str().unwrap().to_string();

        // Bob beitreten + genehmigen, aber NICHT abstimmen
        post_json(&app, "/join", json!({
            "session_id": session_id, "participant_id": "bob"
        })).await;
        post_json(&app, "/approve", json!({
            "session_id": session_id, "creator_id": "alice",
            "participant_id": "bob", "approved": true
        })).await;

        // Ergebnisse abrufen → sollte ready: false zurückgeben
        let (status, body) = get_json(
            &app,
            &format!("/results/{}/alice", session_id)
        ).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["ready"], false);
        println!("✅ Ergebnisse sind noch nicht bereit (nicht alle haben abgestimmt)");
    }
}
