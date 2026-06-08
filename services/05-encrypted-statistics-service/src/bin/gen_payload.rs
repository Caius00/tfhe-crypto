// src/bin/gen_payload.rs
// Generiert Testpayloads für die k6-Lastests.
// Ausführen mit: cargo run --bin gen_payload
//
// Schreibt folgende Dateien ins aktuelle Verzeichnis:
//   payload_sk.txt             – Base64-kodierter CompressedServerKey
//   payload_list_n5_b8.txt    – JSON-Array mit 5 verschlüsselten FheInt8-Werten
//   payload_list_n10_b8.txt   – JSON-Array mit 10 verschlüsselten FheInt8-Werten
//   payload_list_n10_b16.txt  – JSON-Array mit 10 verschlüsselten FheInt16-Werten

use base64::{engine::general_purpose, Engine as _};
use std::fs;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheInt8, FheInt16};
use tfhe::prelude::*;

fn encrypt_list_i8(values: &[i8], client_key: &ClientKey) -> Vec<String> {
    values.iter().map(|&v| {
        let enc = FheInt8::encrypt(v, client_key);
        general_purpose::STANDARD.encode(bincode::serialize(&enc).unwrap())
    }).collect()
}

fn encrypt_list_i16(values: &[i16], client_key: &ClientKey) -> Vec<String> {
    values.iter().map(|&v| {
        let enc = FheInt16::encrypt(v, client_key);
        general_purpose::STANDARD.encode(bincode::serialize(&enc).unwrap())
    }).collect()
}

fn main() {
    println!("Generiere TFHE-Schlüsselpaar...");
    let config = ConfigBuilder::default().build();
    let client_key = ClientKey::generate(config);
    let server_key = CompressedServerKey::new(&client_key);

    // ServerKey speichern
    let sk_b64 = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    fs::write("payload_sk.txt", &sk_b64).unwrap();
    println!("payload_sk.txt geschrieben ({} bytes)", sk_b64.len());

    // n=5, bit_width=8
    println!("Verschlüssele Liste n=5, bit_width=8...");
    let list_n5_b8 = encrypt_list_i8(&[10, 42, 7, 99, 23], &client_key);
    let json = serde_json::to_string(&list_n5_b8).unwrap();
    fs::write("payload_list_n5_b8.txt", &json).unwrap();
    println!("payload_list_n5_b8.txt geschrieben");

    // n=10, bit_width=8
    println!("Verschlüssele Liste n=10, bit_width=8...");
    let list_n10_b8 = encrypt_list_i8(&[10, 42, 7, 99, 23, 55, 3, 77, 31, 60], &client_key);
    let json = serde_json::to_string(&list_n10_b8).unwrap();
    fs::write("payload_list_n10_b8.txt", &json).unwrap();
    println!("payload_list_n10_b8.txt geschrieben");

    // n=10, bit_width=16
    println!("Verschlüssele Liste n=10, bit_width=16...");
    let list_n10_b16 = encrypt_list_i16(&[1000, 4200, 700, 9900, 2300, 5500, 300, 7700, 3100, 6000], &client_key);
    let json = serde_json::to_string(&list_n10_b16).unwrap();
    fs::write("payload_list_n10_b16.txt", &json).unwrap();
    println!("payload_list_n10_b16.txt geschrieben");

    println!("\nFertig. Alle payload_*.txt Dateien liegen im aktuellen Verzeichnis.");
    println!("k6-Skripte müssen im gleichen Verzeichnis wie die payload_*.txt Dateien liegen.");
}