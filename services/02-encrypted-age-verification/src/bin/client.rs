use base64::{engine::general_purpose, Engine as _};
use reqwest::blocking::Client;
use serde_json::json;
use std::time::Duration;
use tfhe::prelude::*;
use tfhe::{generate_keys, CompressedServerKey, ConfigBuilder, FheBool, FheUint8};

fn main() {
    println!("Generiere Schlüssel (das kann einen Moment dauern)...");

    let config = ConfigBuilder::default().build();
    let (client_key, _) = generate_keys(config);
    let compressed_sk = CompressedServerKey::new(&client_key);
    let server_key_bytes = bincode::serialize(&compressed_sk).unwrap();

    //let server_key_bytes = bincode::serialize(&server_key).unwrap();
    let encoded_sk = general_purpose::STANDARD.encode(&server_key_bytes);

    let client = Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("Failed to build client");

    let test_ages = vec![15u8, 17u8, 18u8, 20u8, 25u8];

    println!("Starte Age Verification Tests:");

    for age_value in test_ages {
        let enc_age = FheUint8::try_encrypt(age_value, &client_key).unwrap();
        let encoded_age = general_purpose::STANDARD.encode(bincode::serialize(&enc_age).unwrap());

        let res = client
            .post("http://127.0.0.1:3000/age-verification")
            .json(&json!({
                "encrypted_age": encoded_age,
                "server_key": encoded_sk
            }))
            .send();

        match res {
            Ok(response) => {
                let status = response.status();
                let text = response.text().unwrap();

                if status.is_success() {
                    let json_res: serde_json::Value = serde_json::from_str(&text).unwrap();
                    let b64_res = json_res["is_adult"].as_str().unwrap();

                    let res_bytes = general_purpose::STANDARD.decode(b64_res).unwrap();
                    let enc_bool: FheBool = bincode::deserialize(&res_bytes).unwrap();
                    let is_adult: bool = enc_bool.decrypt(&client_key);

                    println!(
                        "Input Alter: {} => Darf Vodka kaufen? {}",
                        age_value, is_adult
                    );
                } else {
                    println!(
                        "Server Fehler bei Alter {}: {} - {}",
                        age_value, status, text
                    );
                }
            }
            Err(e) => println!("Verbindungsfehler bei Alter {}: {}", age_value, e),
        }
    }
}
