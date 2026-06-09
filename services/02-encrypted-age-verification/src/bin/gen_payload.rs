// src/bin/gen_payload.rs
// Generiert 10 Schlüsselpaare für den realistischen pro-VU Stresstest.
// Ausführen mit: cargo run --bin gen_payload
//
// Schreibt folgende Dateien ins aktuelle Verzeichnis:
//   payload_vu{1-10}_age.txt  – Base64-kodierter FheInt8 (Alter 20)
//   payload_vu{1-10}_sk.txt   – Base64-kodierter CompressedServerKey

use base64::{engine::general_purpose, Engine as _};
use std::fs;
use tfhe::prelude::*;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheInt8};

fn main() {
    let num_pairs = 10;

    for i in 1..=num_pairs {
        println!("Generiere Schlüsselpaar {}/{}...", i, num_pairs);

        let config = ConfigBuilder::default().build();
        let client_key = ClientKey::generate(config);
        let server_key = CompressedServerKey::new(&client_key);

        let age: i8 = 20;
        let encrypted_age = FheInt8::encrypt(age, &client_key);

        let sk_b64  = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
        let age_b64 = general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

        fs::write(format!("payload_vu{}_sk.txt",  i), &sk_b64).unwrap();
        fs::write(format!("payload_vu{}_age.txt", i), &age_b64).unwrap();

        println!("  payload_vu{}_sk.txt  ({} bytes)", i, sk_b64.len());
        println!("  payload_vu{}_age.txt ({} bytes)", i, age_b64.len());
    }

    println!("\nFertig. {} Schlüsselpaare generiert.", num_pairs);
}