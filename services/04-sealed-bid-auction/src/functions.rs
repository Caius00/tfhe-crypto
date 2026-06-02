use serde::{Deserialize, Serialize};
use tfhe::integer::server_key;
use std::sync::Mutex;
use base64::{engine::general_purpose, Engine as _};
use tfhe::{CompressedServerKey, FheBool, FheUint32};
use tfhe::prelude::*;
use schemars::JsonSchema;
use axum::{Json, http::StatusCode};

// Steckbrief --> client zum Server
#[derive(serde::Deserialize, serde::Serialize)]
pub struct BidRequest {
    pub bidder_name: String,
    pub encrypted_amount: String, 
    pub server_key: String,       
}

#[derive(Serialize, JsonSchema)]
pub struct AuctionResponse {
   pub  status: String,
    pub encrypted_result: String, 
}



// Das Format für ein gebot in server-speicher
#[derive(Clone)]
pub struct Bid {
    pub bidder_name: String,
    pub encrypted_amount: FheUint32,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct StringResponse {
    response: String,
}

// Liste für gebote
pub static BIDS: Mutex<Vec<Bid>> = Mutex::new(Vec::new());

//  empfängt das Gebot und entpackung 
pub async fn gebot_empfangen(Json(req): Json<BidRequest>) -> Result<&'static str, String> {
    // Server-Schlüssel aus Base64 decodieren und dekomprimieren
    let sk_bytes = general_purpose::STANDARD
        .decode(&req.server_key)
        .map_err(|e| format!("Invalid ServerKey Base64: {}", e))?;

    let compressed: CompressedServerKey = bincode::deserialize(&sk_bytes)
        .map_err(|e| format!("Failed to deserialize CompressedServerKey: {}", e))?;

    let server_key = compressed.decompress();

    // Verschlüsseltes Gebot aus Base64 decodieren und deserialisieren
    let bid_bytes = general_purpose::STANDARD
        .decode(&req.encrypted_amount)
        .map_err(|e| format!("Invalid Age Base64: {}", e))?;

    let enc_amount: FheUint32 = bincode::deserialize(&bid_bytes)
        .map_err(|e| format!("Failed to deserialize Encrypted Amount: {}", e))?;

    //  In die Liste eintragen (Server-Key wird hier noch nicht gebraucht, erst beim Vergleichen)
    let neues_gebot = Bid {
        bidder_name: req.bidder_name,
        encrypted_amount: enc_amount,
    };

    let mut liste = BIDS.lock().unwrap();
    liste.push(neues_gebot);

    tokio::task::block_in_place(|| {
        tfhe::set_server_key(server_key);
    });

    Ok("Gebot erfolgreich im Liste gespeichert!")
}

// Auswertung
pub async fn auktion_auswerten() -> Result<Json<AuctionResponse>, String> {
    let liste = BIDS.lock().unwrap();

    if liste.is_empty() {
        return Err("Keine Gebote vorhanden!".to_string());
    }

    let _gewinner_nachricht = tokio::task::block_in_place(|| {
        let mut gewinner_index = 0;

// Wir nehmen einfach ein vorhandenes Gebot als Basis für den FHE-Kontext
let mut finales_ergebnis = liste[0].encrypted_amount.gt(&liste[0].encrypted_amount);

for i in 1..liste.len() {
          
            let _ist_neues_gebot_groesser: FheBool = liste[i]
                .encrypted_amount
                .gt(&liste[gewinner_index].encrypted_amount);
        }
        
        bincode::serialize(&finales_ergebnis).unwrap()
    });

    Ok(Json(AuctionResponse {
        status: format!("Die Auktion mit {} Geboten wurde blind ausgewertet!", liste.len()),
        encrypted_result: general_purpose::STANDARD.encode(_gewinner_nachricht),
    }))
}


pub fn hallo_test() -> Result<Json<StringResponse>, (StatusCode, String)> {
    Ok(Json(StringResponse {    
        response: "Hallo! Der Auktions-Server ist bereit.".to_string()
    }))
}