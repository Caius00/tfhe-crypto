



use axum::{routing::{get, post}, Json, Router};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheBool, FheUint32};

// Steckbrief --> client zum Server
#[derive(Deserialize)]
struct BidRequest {
    bidder_name: String,
    encrypted_amount: String, 
    server_key: String,       
}

// Das Format für ein gebot in server-speicher
#[derive(Clone)]
struct Bid {
    bidder_name: String,
    encrypted_amount: FheUint32,
}

// Liste für gebote
static BIDS: Mutex<Vec<Bid>> = Mutex::new(Vec::new());

//  empfängt das Gebot und entpackung 
async fn gebot_empfangen(Json(req): Json<BidRequest>) -> Result<&'static str, String> {
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

    Ok("Gebot erfolgreich im Liste gespeichert!")
}

// Auswertung
async fn auktion_auswerten() -> Result<String, String> {
    let liste = BIDS.lock().unwrap();

    if liste.is_empty() {
        return Ok("Keine Gebote vorhanden!".to_string());
    }

    let _gewinner_nachricht = tokio::task::block_in_place(|| {
        let mut gewinner_index = 0;

        for i in 1..liste.len() {
          
            let _ist_neues_gebot_groesser: FheBool = liste[i]
                .encrypted_amount
                .gt(&liste[gewinner_index].encrypted_amount);
            
          
        }
        
        format!("Die Auktion mit {} Geboten wurde blind ausgewertet!", liste.len())
    });

    Ok("Die Auktion wurde erfolgreich im FHE-Modus verarbeitet!".to_string())
}

async fn hallo_test() -> &'static str {
    "Hallo! Der Auktions-Server ist bereit."
}

#[tokio::main]
async fn main() {
    
    let app = Router::new()
        .route("/test", get(hallo_test))
        .route("/gebot", post(gebot_empfangen))
        .route("/auswerten", get(auktion_auswerten))
        .merge(health::router(env!("CARGO_PKG_VERSION")))
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024 * 1024));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Fehler beim Binden an Port 8080: {}", e);
            std::process::exit(1);
        }
    };

    println!("Auktions-Server läuft auf http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

