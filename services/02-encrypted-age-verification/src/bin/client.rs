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
        .post("http://127.0.0.1:8000/age-verification")
        .json(&serde_json::json!({
            "encrypted_age": encoded_age,
            "server_key": encoded_sk
        }))
        .send()
        .expect("Request fehlgeschlagen");

    let status = res.status();
    let text = res.text().unwrap();

    if status.is_success() {
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(json) => {
                if let Some(b64_res) = json["is_adult"].as_str() {
                    match general_purpose::STANDARD.decode(b64_res) {
                        Ok(res_bytes) => match bincode::deserialize::<FheBool>(&res_bytes) {
                            Ok(enc_bool) => {
                                let is_adult: bool = enc_bool.decrypt(&client_key);
                                println!("--- Ergebnis ---");
                                println!("Ist die Person volljährig? {}", is_adult);
                            }
                            Err(e) => println!("Fehler beim Deserialisieren der Response: {}", e),
                        },
                        Err(e) => println!("Fehler beim Base64-Dekodieren: {}", e),
                    }
                } else {
                    println!("Ungültige Response-Struktur: {}", text);
                }
            }
            Err(e) => println!(
                "Response ist nicht gültiges JSON: {} \nResponse-Text: {}",
                e, text
            ),
        }
    } else {
        println!("Server Fehler (Status {}): {}", status, text);
    }
}
