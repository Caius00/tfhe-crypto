use base64::{engine::general_purpose, Engine as _};
use reqwest::blocking::Client;
use std::time::Duration;
use tfhe::prelude::*;
use tfhe::{generate_keys, ConfigBuilder, FheBool, FheUint8};

fn main() {
    let config = ConfigBuilder::default().build();
    let (client_key, server_key) = generate_keys(config);

    let age_value = 20u8;
    let enc_age = FheUint8::try_encrypt(age_value, &client_key).unwrap();

    let encoded_age = general_purpose::STANDARD.encode(bincode::serialize(&enc_age).unwrap());
    let encoded_sk = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());

    let client = Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("Failed to build client");

    let res = client
        .post("http://127.0.0.1:8080/age-verification")
        .json(&serde_json::json!({
            "encrypted_age": encoded_age,
            "server_key": encoded_sk
        }))
        .send()
        .expect("Request fehlgeschlagen");

    if res.status().is_success() {
        let json: serde_json::Value = res.json().unwrap();
        let b64_res = json["is_adult"].as_str().unwrap();

        let res_bytes = general_purpose::STANDARD.decode(b64_res).unwrap();
        let enc_bool: FheBool = bincode::deserialize(&res_bytes).unwrap();
        let is_adult: bool = enc_bool.decrypt(&client_key);

        println!("--- Ergebnis ---");
        println!("Ist die Person volljährig? {}", is_adult);
    } else {
        println!("Server Fehler: {}", res.text().unwrap());
    }
}
