use tfhe::boolean::backward_compatibility::{client_key, server_key};
use tfhe::{ConfigBuilder, FheBool, FheUint32, generate_keys, set_server_key};
use tfhe::prelude::*;
use axum::Router;
use axum::Json;
use serde::{Deserialize, Serialize};


fn bestimme_gewinner(gebot_a: &FheUint32, gebot_b: &FheUint32) -> FheBool {
    gebot_a.gt(gebot_b)
}

#[derive(Serialize, Deserialize, Clone)]
struct Bid {
    bidder_name: String,
    encrypted_amount: FheUint32,
}

async fn gebot_empfangen(Json(neues_gebot): Json<Bid>) -> &'static str {
    let mut liste = BIDS.lock().unwrap();

    liste.push(neues_gebot);
    "Gebot erfolgreich im Tresor gespeichert!"
}

async fn auktion_auswerten() -> String {
    let liste = BIDS.lock().unwrap();

    if liste.is_empty() {
        return "Keine Gebote vorhanden!".to_string();
    }

    let mut gewinner_index = 0;
    for i in 1..liste.len() {
        let ist_neues_gebot_groesser = liste[i].encrypted_amount.gt(&liste[gewinner_index].encrypted_amount);
    }
    format!("Die Auktion wurde ausgewertet!")
}

async fn hallo_test() -> &'static str {
    "Hallo! Der Auktions-Server ist bereit."
}

use std::sync::Mutex;
static BIDS: Mutex<Vec<Bid>> = Mutex::new(Vec::new());
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = ConfigBuilder::default().build();
    let (client_key, server_key) = generate_keys(config);
    set_server_key(server_key);

    // Ein Gebot von 100 Euro verschlüsseln
    let secret_bid_value = FheUint32::encrypt(100u32, &client_key);
    



    let app = Router::new()
        .route("/test", axum::routing::get(hallo_test))
        .route("/auswerten", axum::routing::get(auktion_auswerten))
        .route("/gebot", axum::routing::post(gebot_empfangen))
        .merge(health::router(env!("CARGO_PKG_VERSION")));

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 8080));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();

    //Test
    let secret_bid_2 = FheUint32::encrypt(150u32, &client_key);
    let is_a_greater = bestimme_gewinner(&secret_bid_value, &secret_bid_2);
    let result: bool = is_a_greater.decrypt(&client_key);
    if result {
        println!("Gebot A was greater!");
    } else {
        println!("Gebot B was greater!");
    }
    Ok(())
}

