use super::functions::*;
use super::*;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use base64::{engine::general_purpose, Engine as _};
use serial_test::serial;
use std::sync::OnceLock;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheBool, FheUint32};
use tower::ServiceExt;
use crate::functions::*; 
use crate::*;  
use tfhe::prelude::*;           



fn clear_bids() {
    let mut liste = BIDS.lock().unwrap();
    liste.clear();
}


static TEST_SETUP: OnceLock<(ClientKey, CompressedServerKey)> = OnceLock::new();

fn get_test_setup() -> &'static (ClientKey, CompressedServerKey) {
    TEST_SETUP.get_or_init(|| {
        let config = ConfigBuilder::default().build();
        let ck = ClientKey::generate(config);
        let sk = CompressedServerKey::new(&ck);
        (ck, sk)
    })
}


#[tokio::test]
async fn test_gebot_empfangen_invalid_base64() {
    clear_bids();
    let app = Router::new().route("/gebot", axum::routing::post(gebot_empfangen));

    // Payload
    let payload = BidRequest {
        bidder_name: "Tammo".to_string(),
        encrypted_amount: "kein-valides-base64!!!".to_string(),
        server_key: "auch-falsch!!".to_string(),
    };

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/gebot")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    // kein Server-Crash (HTTP 500)
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// gegen korrupte Binärdaten (Deserialisierungsfehle
#[tokio::test]
async fn test_gebot_empfangen_corrupt_bytes() {
    clear_bids();
    let app = Router::new().route("/gebot", axum::routing::post(gebot_empfangen));

    // Valides Base64, der Inhalt repäsentiert jedoch ein simples, flaches Array
    let corrupt_payload = general_purpose::STANDARD.encode(vec![1, 2, 3, 4, 5]);

    let payload = BidRequest {
        bidder_name: "Sarah".to_string(),
        encrypted_amount: corrupt_payload.clone(),
        server_key: corrupt_payload,
    };

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/gebot")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    //  Bincode-Fehler wird abgefangen -> HTTP 400
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// wenn eine auswertung gemacht wird ohne gebote  -->  system entwirft hinweis
#[test]
#[serial]
fn test_auktion_leere_liste() {
    clear_bids();
    
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let result = runtime.block_on(auktion_auswerten());
    
    
assert!(result.is_err(), "Die Auswertung hätte bei einer leeren Liste fehlschlagen müssen!");}




#[tokio::test(flavor = "multi_thread")]
#[serial] 
async fn test_auktion_full_roundtrip() {
  


    clear_bids();
    let (client_key, server_key) = get_test_setup();

    let app = Router::new()
        .route("/gebot", axum::routing::post(gebot_empfangen))
        .route("/auswerten", axum::routing::get(auktion_auswerten));

    // Zwei Test-Gebote festlegen
    let bid_a: u32 = 100;
    let bid_b: u32 = 150;
    
    // Verschlüsselung auf Client-Seite simulieren
   let enc_bid_a = FheUint32::try_encrypt(bid_a, client_key).unwrap();
let enc_bid_b = FheUint32::try_encrypt(bid_b, client_key).unwrap();

    // Pachet for HTTP-Transport via Base64 & Bincode
    let sk_payload = general_purpose::STANDARD.encode(bincode::serialize(server_key).unwrap());
    
    let payload_a = BidRequest {
        bidder_name: "Bieter_A_100".to_string(),
        encrypted_amount: general_purpose::STANDARD.encode(bincode::serialize(&enc_bid_a).unwrap()),
        server_key: sk_payload.clone(),
    };
    
    let payload_b = BidRequest {
        bidder_name: "Bieter_B_150".to_string(),
        encrypted_amount: general_purpose::STANDARD.encode(bincode::serialize(&enc_bid_b).unwrap()),
        server_key: sk_payload,
    };


    //  Gebot A zu server
    let response_a = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/gebot")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload_a).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response_a.status(), StatusCode::OK);

    // Bebot B ans Servere
    let response_b = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/gebot")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload_b).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response_b.status(), StatusCode::OK);

    
    
    // Server anweisen zu vergleichen
    let response_eval = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/auswerten")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    
    assert_eq!(response_eval.status(), StatusCode::OK);

    // Beide Objects in server gespeichert?
    let liste = BIDS.lock().unwrap();
    assert_eq!(liste.len(), 2);

    // Verifikations-Check: Client-Key --> Simulierter Client
    // Ergebnis des Servers entschlüsseln.
    tfhe::set_server_key(server_key.decompress());
   
let ist_b_groesser: FheBool = (&liste[1].encrypted_amount).gt(&liste[0].encrypted_amount);


let ergebnis: bool = ist_b_groesser.decrypt(&client_key);
    
    assert!(ergebnis, "Mathematischer FHE-Check fehlgeschlagen: Gebot B (150) muss als größer als Gebot A (100) evaluiert werden!");
}