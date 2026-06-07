pub mod logic;
pub mod types;

use self::types::{ApiResult, AuctionResponse, Bid, BidRequest, StringResponse};
use axum::http::StatusCode;
use axum::Json;
use base64::{engine::general_purpose, Engine as _};
use std::sync::Mutex;

pub static BIDS: Mutex<Vec<Bid>> = Mutex::new(Vec::new());

/// POST /gebot
pub async fn gebot_empfangen(axum::Json(req): axum::Json<BidRequest>) -> ApiResult<StringResponse> {
    // 1. Server-Key decodieren
    let sk_bytes = general_purpose::STANDARD
        .decode(&req.server_key)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid ServerKey Base64: {}", e),
            )
        })?;

    // 2. Den Key kurz aktivieren, damit TFHE das Gebot decompressen/deserialisieren darf
    let compressed: tfhe::CompressedServerKey = bincode::deserialize(&sk_bytes).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("Failed to deserialize ServerKey: {}", e),
        )
    })?;
    let server_key = compressed.decompress();
    tfhe::set_server_key(server_key);

    // 3. Verschlüsseltes Gebot decodieren
    let bid_bytes = general_purpose::STANDARD
        .decode(&req.encrypted_amount)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid Bid Base64: {}", e),
            )
        })?;

    let enc_amount: tfhe::FheUint32 = match bincode::deserialize(&bid_bytes) {
        Ok(fhe_type) => fhe_type,
        Err(e) => {
            return Err((
                StatusCode::BAD_REQUEST,
                format!(
                    "Failed to deserialize Encrypted Amount into TFHE type: {}",
                    e
                ),
            ))?;
        }
    };
    // 5. Gebot zusammen mit den sk_bytes sichern!
    let neues_gebot = Bid {
        bidder_name: req.bidder_name,
        encrypted_amount: enc_amount,
        server_key_bytes: sk_bytes,
    };

    BIDS.lock().unwrap().push(neues_gebot);

    Ok(Json(StringResponse {
        response: "Gebot erfolgreich empfangen!".to_string(),
    }))
}

/// GET /auswerten
pub async fn auktion_auswerten() -> ApiResult<AuctionResponse> {
    let liste = BIDS.lock().unwrap();
    if liste.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "Keine Gebote vorhanden".to_string(),
        ));
    }

    // echtem Key aus dem allerersten Gebot
    let real_sk_bytes = &liste[0].server_key_bytes;

    // Aufruf der Berechnungslogik
    let gewinner_nachricht = logic::evaluate_encrypted_auction(&liste, real_sk_bytes);

    Ok(Json(AuctionResponse {
        status: format!(
            "Die Auktion mit {} Geboten wurde blind ausgewertet!",
            liste.len()
        ),
        encrypted_result: general_purpose::STANDARD.encode(gewinner_nachricht),
    }))
}

/// GET /hallo
pub async fn hallo_test() -> Json<StringResponse> {
    Json(StringResponse {
        response: "Hallo aus der FHE-Auktion!".to_string(),
    })
}
