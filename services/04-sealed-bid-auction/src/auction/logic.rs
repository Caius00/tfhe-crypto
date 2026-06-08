use crate::auction::types::Bid;
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheUint32, ServerKey}; // <-- Hier FheUint32 importiert!

pub fn evaluate_encrypted_auction(liste: &[Bid], server_key_bytes: &[u8]) -> Vec<u8> {
    if server_key_bytes.is_empty() {
        panic!("DIE LOGIC HAT EINEN LEEREN SERVER-KEY ERHALTEN!");
    }
    if liste.is_empty() {
        panic!("DIE GEBOTS-LISTE IST LEER!");
    }

    let compressed_key: CompressedServerKey =
        bincode::deserialize(server_key_bytes).expect("Failed to deserialize server key");
    let server_key: ServerKey = compressed_key.decompress();
    tfhe::set_server_key(server_key);

    let mut maximales_gebot: FheUint32 = liste[0].encrypted_amount.clone();

    for naechstes_gebot in liste.iter().skip(1) {
        let ist_groesser = naechstes_gebot.encrypted_amount.gt(&maximales_gebot);

        maximales_gebot = ist_groesser.select(&naechstes_gebot.encrypted_amount, &maximales_gebot);
    }

    bincode::serialize(&maximales_gebot).unwrap()
}
