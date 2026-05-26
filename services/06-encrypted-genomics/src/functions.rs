use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use tfhe::prelude::*;
use tfhe::{
    generate_keys, set_server_key, ClientKey, ConfigBuilder, FheUint8, PublicKey, ServerKey,
};

#[derive(Clone)]
pub struct AppState {
    pub client_key: ClientKey,
    pub server_key: ServerKey,
    pub public_key: PublicKey,
}

impl AppState {
    pub fn new() -> Self {
        let config = ConfigBuilder::default().build();
        let (client_key, server_key) = generate_keys(config);
        let public_key = PublicKey::new(&client_key);
        // Globalen Server‑Key für Rayon setzen
        rayon::broadcast(|_| set_server_key(server_key.clone()));
        set_server_key(server_key.clone());
        AppState {
            client_key,
            server_key,
            public_key,
        }
    }
}

pub fn encode_dna(seq: &str) -> Result<Vec<u8>, String> {
    seq.to_uppercase()
        .chars()
        .map(|c| match c {
            'A' => Ok(0u8),
            'T' => Ok(1u8),
            'C' => Ok(2u8),
            'G' => Ok(3u8),
            other => Err(format!("illegal base '{}' in sequence (ಠ_ಠ)", other)),
        })
        .collect()
}

pub fn parse_pattern(pattern_str: &str) -> Result<Vec<u8>, String> {
    pattern_str
        .chars()
        .map(|c| {
            c.to_digit(10)
                .ok_or_else(|| format!("Kein Ziffernzeichen: '{}'", c))
                .and_then(|d| {
                    if d <= 3 {
                        Ok(d as u8)
                    } else {
                        Err(format!("Letter {} not allowed (A,T,G,C)~0-3 (ノ°益°)ノ", d))
                    }
                })
        })
        .collect()
}

fn homomorphic_hamming_distance(
    window: &[FheUint8],
    enc_pattern: &[FheUint8],
    public_key: &PublicKey,
) -> FheUint8 {
    let diffs: Vec<FheUint8> = window
        .iter()
        .zip(enc_pattern.iter())
        .map(|(w, p)| {
            let ne = w.ne(p);
            FheUint8::cast_from(ne)
        })
        .collect();
    if diffs.is_empty() {
        FheUint8::encrypt(0u8, public_key)
    } else {
        let mut acc = diffs[0].clone();
        for diff in &diffs[1..] {
            acc = &acc + diff;
        }
        acc
    }
}

pub fn homomorphic_sliding_window(
    enc_seq: &[FheUint8],
    enc_pattern: &[FheUint8],
    public_key: &PublicKey,
) -> Vec<FheUint8> {
    let n = enc_seq.len();
    let m = enc_pattern.len();
    if m > n {
        panic!("pattern > sequence (ノಠ益ಠ)ノ彡┻━┻");
    }
    (0..=(n - m))
        .map(|start| {
            let window = &enc_seq[start..start + m];
            homomorphic_hamming_distance(window, enc_pattern, public_key)
        })
        .collect()
}

pub fn serialize_fhe_vec(data: &[FheUint8]) -> Vec<u8> {
    bincode::serialize(&data.to_vec()).expect("serializing failed (ง'̀-'́)ง")
}

pub fn deserialize_fhe_vec(bytes: &[u8]) -> Vec<FheUint8> {
    bincode::deserialize(bytes).expect("deserialiazing failed ᕙ(̀-'́)ᕗ")
}

// API DTOs
#[derive(Deserialize, Serialize, JsonSchema)]
pub struct EncryptRequest {
    pub sequence: String,
}

#[derive(Serialize, JsonSchema)]
pub struct EncryptResponse {
    pub encrypted_data: String,
    pub original_length: usize,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct ProcessRequest {
    pub encrypted_sequence: String,
    pub risk_pattern: String,
}

#[derive(Serialize, JsonSchema)]
pub struct ProcessResponse {
    pub encrypted_distances: String,
    pub windows: usize,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct DecryptRequest {
    pub encrypted_data: String,
}

#[derive(Serialize, JsonSchema)]
pub struct DecryptResponse {
    pub plain_data: Vec<u8>,
}

// API Handler
pub async fn encrypt_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(req): axum::Json<EncryptRequest>,
) -> Result<axum::Json<EncryptResponse>, (axum::http::StatusCode, String)> {
    let clean = encode_dna(&req.sequence).map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e))?;
    let len = clean.len();

    let now = Instant::now();
    let encrypted: Vec<FheUint8> = clean
        .par_iter()
        .map(|&b| FheUint8::try_encrypt(b, &state.client_key).unwrap())
        .collect();
    println!("encryption finished in {:?}", now.elapsed());

    let bytes = serialize_fhe_vec(&encrypted);
    let b64 = BASE64.encode(&bytes);
    Ok(axum::Json(EncryptResponse {
        encrypted_data: b64,
        original_length: len,
    }))
}

pub async fn process_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(req): axum::Json<ProcessRequest>,
) -> Result<axum::Json<ProcessResponse>, (axum::http::StatusCode, String)> {
    set_server_key(state.server_key.clone());

    let now = Instant::now();

    let enc_bytes = BASE64.decode(&req.encrypted_sequence).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("Base64-Fehler: {}", e),
        )
    })?;

    let enc_seq = deserialize_fhe_vec(&enc_bytes);

    let pattern =
        parse_pattern(&req.risk_pattern).map_err(|e| (axum::http::StatusCode::BAD_REQUEST, e))?;

    let enc_pattern: Vec<FheUint8> = pattern
        .iter()
        .map(|&b| FheUint8::try_encrypt(b, &state.public_key).unwrap())
        .collect();

    let enc_distances = homomorphic_sliding_window(&enc_seq, &enc_pattern, &state.public_key);

    println!("processing finished in {:?}", now.elapsed());

    let windows = enc_distances.len();

    let dist_bytes = serialize_fhe_vec(&enc_distances);
    let dist_b64 = BASE64.encode(&dist_bytes);

    Ok(axum::Json(ProcessResponse {
        encrypted_distances: dist_b64,
        windows,
    }))
}

pub async fn decrypt_handler(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
    axum::Json(req): axum::Json<DecryptRequest>,
) -> Result<axum::Json<DecryptResponse>, (axum::http::StatusCode, String)> {
    let now = Instant::now();

    let enc_bytes = BASE64.decode(&req.encrypted_data).map_err(|e| {
        (
            axum::http::StatusCode::BAD_REQUEST,
            format!("Base64-err: {}", e),
        )
    })?;
    let enc_vec = deserialize_fhe_vec(&enc_bytes);

    let plain: Vec<u8> = enc_vec
        .par_iter()
        .map(|d| d.decrypt(&state.client_key))
        .collect();

    println!("decryption finished in {:?}", now.elapsed());

    Ok(axum::Json(DecryptResponse { plain_data: plain }))
}
