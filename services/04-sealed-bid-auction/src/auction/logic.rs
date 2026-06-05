use crate::auction::types::Bid;
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheUint32, ServerKey};

pub fn evaluate_encrypted_auction(liste: &[Bid], server_key_bytes: &[u8]) -> Vec<u8> {
    if server_key_bytes.is_empty() {
        panic!("DIE LOGIC HAT EINEN LEEREN SERVER-KEY ERHALTEN!");
    }
    let compressed_key: CompressedServerKey =
        bincode::deserialize(server_key_bytes).expect("Failed to deserialize server key");
    let server_key: ServerKey = compressed_key.decompress();
    tfhe::set_server_key(server_key);

    let finales_ergebnis = (&liste[1].encrypted_amount).gt(&liste[0].encrypted_amount);

    bincode::serialize(&finales_ergebnis).unwrap()
}
