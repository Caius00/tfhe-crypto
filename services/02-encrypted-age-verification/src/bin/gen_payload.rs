use base64::{engine::general_purpose, Engine as _};
use tfhe::prelude::*;
use tfhe::{ClientKey, CompressedServerKey, ConfigBuilder, FheInt8};

fn main() {
    let config = ConfigBuilder::default().build();
    let client_key = ClientKey::generate(config);
    let server_key = CompressedServerKey::new(&client_key);

    let age: i8 = 20;
    let encrypted_age = FheInt8::encrypt(age, &client_key);

    let server_key_b64 = general_purpose::STANDARD.encode(bincode::serialize(&server_key).unwrap());
    let encrypted_age_b64 =
        general_purpose::STANDARD.encode(bincode::serialize(&encrypted_age).unwrap());

    println!("ENCRYPTED_AGE={}", encrypted_age_b64);
    println!("SERVER_KEY={}", server_key_b64);
}
