use tfhe::{ConfigBuilder, generate_keys, set_server_key, FheUint8};
use tfhe::prelude::*;
use base64::{Engine, engine::general_purpose::STANDARD};

fn main() {
    println!("Keys generieren...");
    let config = ConfigBuilder::default().build();
    let (client_key, server_key) = generate_keys(config);
    set_server_key(server_key);
    println!("Bereit.");

    // Vote für Rust (Index 0 von 3 Optionen)
    let choices: Vec<String> = (0..3).map(|i| {
        let value: u8 = if i == 0 { 1 } else { 0 };
        let encrypted = FheUint8::encrypt(value, &client_key);
        let bytes = bincode::serialize(&encrypted).unwrap();
        STANDARD.encode(&bytes)
    }).collect();

    let encrypted_name = FheUint8::encrypt(b'A', &client_key);
    let name_bytes = bincode::serialize(&encrypted_name).unwrap();
    let encrypted_name_b64 = STANDARD.encode(&name_bytes);

    let body = serde_json::json!({
        "session_id": 1,
        "voter_id": "voter_1",
        "encrypted_name": encrypted_name_b64,
        "choices": choices
    });

    println!("\nJSON Body:\n{}", serde_json::to_string_pretty(&body).unwrap());
    std::fs::write("vote_body.json", serde_json::to_string(&body).unwrap()).unwrap();
    println!("Body gespeichert in vote_body.json");

 
}