use std::collections::HashMap;
use std::error::Error;
use tfhe::{ClientKey, FheAsciiString};
use tfhe::prelude::{FheTryEncrypt,};
// use rayon::prelude::*;
use crate::custom_fhe_ascii_string::{CustomFheAsciiString, SerializedCustomFheAsciiString};

struct ActiveKeys {
    keys: HashMap<String, SerializedCustomFheAsciiString>,
}

impl ActiveKeys {
    fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }
    fn insert(&mut self, string: String, key: SerializedCustomFheAsciiString) {
        self.keys.insert(string, key);
    }
    fn get(&self, string: &str) -> Option<SerializedCustomFheAsciiString> {
        self.keys.get(string).cloned()
    }
    fn remove(&mut self, string: &String) {
        self.keys.remove(string);
    }
}

fn frontend_put(client_key: &ClientKey, key: &str, value: &str, active_keys: &mut ActiveKeys) -> Result<(), Box<dyn Error>>{
    let enc_key = CustomFheAsciiString::new(key, client_key);
    let enc_val = FheAsciiString::try_encrypt(value, client_key)?;

    let serialized_key = enc_key.serialize();
    let serialized_val = bincode::serialize(&enc_val)?;
    serialized_val.len();

    active_keys.insert(key.to_string(), serialized_key);

    // call api
    Ok(())
}