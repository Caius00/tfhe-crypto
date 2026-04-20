use base64::{engine::general_purpose, Engine as _};
use reqwest::blocking::Client;
use serde_json::json;
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tfhe::prelude::*;
use tfhe::{generate_keys, ConfigBuilder, FheBool, FheUint8};

fn main() {
    let local_keys_dir = "./keys";
    let workspace_keys_dir = "../../keys";

    let keys_dir = if Path::new(workspace_keys_dir).exists() {
        workspace_keys_dir
    } else {
        local_keys_dir
    };

    let client_key_path = Path::new(keys_dir).join("client_key.bin");
    let server_key_path = Path::new(keys_dir).join("server_key.bin");

    let args: Vec<String> = env::args().collect();
    let generate_mode = args.contains(&"--generate-keys".to_string());

    if generate_mode {
        fs::create_dir_all(keys_dir).expect("Failed to create keys directory");

        let config = ConfigBuilder::default().build();
        let (client_key, server_key) = generate_keys(config);

        let client_key_bytes = bincode::serialize(&client_key).unwrap();
        fs::write(&client_key_path, &client_key_bytes).expect("Failed to save ClientKey");

        let server_key_bytes = bincode::serialize(&server_key).unwrap();
        fs::write(&server_key_path, &server_key_bytes).expect("Failed to save ServerKey");

        return;
    }

    if !client_key_path.exists() {
        eprintln!("ClientKey not found");
        eprintln!("Run: cargo run --bin client -- --generate-keys");
        std::process::exit(1);
    }

    let client_key_bytes = fs::read(&client_key_path).expect("Failed to read ClientKey");
    let client_key: tfhe::ClientKey =
        bincode::deserialize(&client_key_bytes).expect("Failed to deserialize ClientKey");

    let server_key_bytes = fs::read(&server_key_path).expect("Failed to read ServerKey");
    let encoded_sk = general_purpose::STANDARD.encode(&server_key_bytes);

    let client = Client::builder()
        .timeout(Duration::from_secs(600))
        .build()
        .expect("Failed to build client");

    let test_ages = vec![15u8, 17u8, 18u8, 20u8, 25u8];

    println!("Age Verification Tests:");

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
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(json) => {
                            if let Some(b64_res) = json["is_adult"].as_str() {
                                match general_purpose::STANDARD.decode(b64_res) {
                                    Ok(res_bytes) => {
                                        match bincode::deserialize::<FheBool>(&res_bytes) {
                                            Ok(enc_bool) => {
                                                let is_adult: bool = enc_bool.decrypt(&client_key);
                                                println!(
                                                    "Age {}: is_adult={}",
                                                    age_value, is_adult
                                                );
                                            }
                                            Err(_) => {
                                                println!("Age {}: deserialization error", age_value)
                                            }
                                        }
                                    }
                                    Err(_) => println!("Age {}: base64 decode error", age_value),
                                }
                            }
                        }
                        Err(_) => println!("Age {}: response error", age_value),
                    }
                } else {
                    println!("Age {}: server error {}", age_value, status);
                }
            }
            Err(_) => println!("Age {}: request error", age_value),
        }
    }
}
