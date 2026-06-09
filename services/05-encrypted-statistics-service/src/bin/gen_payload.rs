// src/bin/gen_payload.rs
// Generiert Testpayloads für die k6-Lasttests.
// Ausführen mit: cargo run --bin gen_payload
// (Arbeitsverzeichnis muss src/Load-Tests/ sein, damit die Dateien dort landen)
//
// Schreibt folgende Dateien ins aktuelle Verzeichnis:
//   payload_sk.txt             – Base64-kodierter CompressedServerKey
//   payload_list_n5_b8.txt    – JSON-Array mit 5 verschlüsselten FheInt8-Werten
//   payload_list_n10_b8.txt   – JSON-Array mit 10 verschlüsselten FheInt8-Werten
//   payload_list_n10_b16.txt  – JSON-Array mit 10 verschlüsselten FheInt16-Werten
//   payload_list_n10_b32.txt  – JSON-Array mit 10 verschlüsselten FheInt32-Werten

use base64::{engine::general_purpose, Engine as _};
use std::fs;
use tfhe::prelude::*;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheInt8, FheInt16, FheInt32};

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

fn encrypt_list_i32(values: &[i32], client_key: &ClientKey) -> Vec<String> {
    values.iter().map(|&v| {
        let enc = FheInt32::encrypt(v, client_key);
        general_purpose::STANDARD.encode(bincode::serialize(&enc).unwrap())
    }).collect()
}

fn main() {
    println!("Generiere TFHE-Schlüsselpaar...");
    let config = ConfigBuilder::default().build();
    let client_key = ClientKey::generate(config);
    let server_key = CompressedServerKey::new(&client_key);

    let sk_b64 = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    fs::write("payload_sk.txt", &sk_b64).unwrap();
    println!("payload_sk.txt geschrieben ({} bytes)", sk_b64.len());

    println!("Verschlüssele Liste n=5, bit_width=8...");
    let list = encrypt_list_i8(&[10, 42, 7, 99, 23], &client_key);
    fs::write("payload_list_n5_b8.txt", serde_json::to_string(&list).unwrap()).unwrap();
    println!("payload_list_n5_b8.txt geschrieben");

    println!("Verschlüssele Liste n=10, bit_width=8...");
    let list = encrypt_list_i8(&[10, 42, 7, 99, 23, 55, 3, 77, 31, 60], &client_key);
    fs::write("payload_list_n10_b8.txt", serde_json::to_string(&list).unwrap()).unwrap();
    println!("payload_list_n10_b8.txt geschrieben");

    println!("Verschlüssele Liste n=10, bit_width=16...");
    let list = encrypt_list_i16(&[1000, 4200, 700, 9900, 2300, 5500, 300, 7700, 3100, 6000], &client_key);
    fs::write("payload_list_n10_b16.txt", serde_json::to_string(&list).unwrap()).unwrap();
    println!("payload_list_n10_b16.txt geschrieben");

    println!("Verschlüssele Liste n=10, bit_width=32...");
    let list = encrypt_list_i32(&[100_000, 420_000, 70_000, 990_000, 230_000, 550_000, 30_000, 770_000, 310_000, 600_000], &client_key);
    fs::write("payload_list_n10_b32.txt", serde_json::to_string(&list).unwrap()).unwrap();
    println!("payload_list_n10_b32.txt geschrieben");

    println!("\nFertig. Alle payload_*.txt Dateien liegen im aktuellen Verzeichnis.");
    println!("Ausführen von src/Load-Tests/: cargo run --bin gen_payload");
}
