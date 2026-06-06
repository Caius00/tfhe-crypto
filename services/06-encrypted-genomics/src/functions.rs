use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;
use tfhe::prelude::*;
use tfhe::{
    set_server_key, CompactCiphertextList, CompactPublicKey, CompressedServerKey, FheUint8,
    ServerKey,
};
use tokio_postgres::NoTls;

type ApiError = (axum::http::StatusCode, String);

const SERVER_RISK_PATTERN: &str = "ATC";
const SEEDED_RISK_PATTERNS: [&str; 3] = ["ATC", "GGTT", "TA"];
const DEFAULT_DATABASE_CONFIG: &str =
    "host=159.195.145.100 port=5432 user=genomics password=genomics dbname=genomics";
const PARALLEL_HAMMING_WINDOWS: usize = 2;
const PARALLEL_LEVENSHTEIN_CELLS: usize = 2;

static FHE_SERVER_KEY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct RiskPattern {
    pub id: i32,
    pub sequence: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct GenomicsSession {
    pub id: String,
    pub public_key: String,
    pub created_at: String,
}

#[derive(Clone)]
struct SessionKeys {
    public_key: String,
    server_key: String,
}

#[derive(Clone)]
struct EncryptedRiskPattern {
    pattern: RiskPattern,
    encrypted_bases: Vec<FheUint8>,
}

struct FunctionTimer {
    function_name: &'static str,
    started: Instant,
}

impl FunctionTimer {
    fn start(function_name: &'static str) -> Self {
        println!("{function_name} started");
        Self {
            function_name,
            started: Instant::now(),
        }
    }
}

impl Drop for FunctionTimer {
    fn drop(&mut self) {
        println!(
            "{} finished in {}ms",
            self.function_name,
            self.started.elapsed().as_millis()
        );
    }
}

fn bad_request(message: impl Into<String>) -> ApiError {
    (axum::http::StatusCode::BAD_REQUEST, message.into())
}

fn internal_error(message: impl Into<String>) -> ApiError {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        message.into(),
    )
}

fn join_error(error: tokio::task::JoinError) -> ApiError {
    internal_error(format!("Blocking task failed: {error}"))
}

fn database_config() -> String {
    std::env::var("GENOMICS_DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_CONFIG.to_string())
}

async fn connect_database() -> Result<tokio_postgres::Client, ApiError> {
    let _timer = FunctionTimer::start("connect_database");
    let config = database_config();
    let (client, connection) = tokio_postgres::connect(config.as_str(), NoTls)
        .await
        .map_err(|e| internal_error(format!("Failed to connect to genomics database: {e}")))?;

    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("Genomics database connection failed: {error}");
        }
    });

    Ok(client)
}

pub async fn initialize_database() -> Result<(), ApiError> {
    let _timer = FunctionTimer::start("initialize_database");
    let client = connect_database().await?;

    client
        .batch_execute(
            "
            CREATE TABLE IF NOT EXISTS risk_patterns (
                id SERIAL PRIMARY KEY,
                sequence TEXT NOT NULL UNIQUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            );

            CREATE TABLE IF NOT EXISTS genomics_sessions (
                id TEXT PRIMARY KEY,
                public_key TEXT NOT NULL,
                server_key TEXT NOT NULL,
                active BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            );
            ",
        )
        .await
        .map_err(|e| internal_error(format!("Failed to create risk_patterns table: {e}")))?;

    for sequence in SEEDED_RISK_PATTERNS {
        client
            .execute(
                "INSERT INTO risk_patterns (sequence) VALUES ($1) ON CONFLICT (sequence) DO NOTHING",
                &[&sequence],
            )
            .await
            .map_err(|e| internal_error(format!("Failed to seed risk pattern {sequence}: {e}")))?;
    }

    Ok(())
}

async fn create_session(
    public_key: String,
    server_key: String,
) -> Result<GenomicsSession, ApiError> {
    let _timer = FunctionTimer::start("create_session");
    initialize_database().await?;
    let client = connect_database().await?;
    let id = uuid::Uuid::new_v4().to_string();

    let row = client
        .query_one(
            "INSERT INTO genomics_sessions (id, public_key, server_key) VALUES ($1, $2, $3) \
             RETURNING created_at::TEXT AS created_at",
            &[&id, &public_key, &server_key],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to create genomics session: {e}")))?;

    Ok(GenomicsSession {
        id,
        public_key,
        created_at: row.get("created_at"),
    })
}

async fn list_sessions() -> Result<Vec<GenomicsSession>, ApiError> {
    let _timer = FunctionTimer::start("list_sessions");
    initialize_database().await?;
    let client = connect_database().await?;
    let rows = client
        .query(
            "SELECT id, public_key, created_at::TEXT AS created_at \
             FROM genomics_sessions WHERE active = TRUE ORDER BY created_at DESC",
            &[],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to list genomics sessions: {e}")))?;

    Ok(rows
        .into_iter()
        .map(|row| GenomicsSession {
            id: row.get("id"),
            public_key: row.get("public_key"),
            created_at: row.get("created_at"),
        })
        .collect())
}

async fn load_session_keys(session_id: &str) -> Result<SessionKeys, ApiError> {
    let _timer = FunctionTimer::start("load_session_keys");
    initialize_database().await?;
    let client = connect_database().await?;
    let row = client
        .query_opt(
            "SELECT public_key, server_key FROM genomics_sessions WHERE id = $1 AND active = TRUE",
            &[&session_id],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to load genomics session: {e}")))?
        .ok_or_else(|| bad_request(format!("Unknown genomics session: {session_id}")))?;

    Ok(SessionKeys {
        public_key: row.get("public_key"),
        server_key: row.get("server_key"),
    })
}

async fn resolve_public_key(
    session_id: Option<&str>,
    provided_public_key: Option<&str>,
) -> Result<String, ApiError> {
    if let Some(public_key) = provided_public_key.filter(|value| !value.trim().is_empty()) {
        return Ok(public_key.to_string());
    }

    let session_id =
        session_id.ok_or_else(|| bad_request("Request needs session_id or public_key"))?;
    Ok(load_session_keys(session_id).await?.public_key)
}

async fn resolve_session_keys(
    session_id: Option<&str>,
    provided_public_key: Option<&str>,
    provided_server_key: Option<&str>,
) -> Result<SessionKeys, ApiError> {
    if let Some(session_id) = session_id.filter(|value| !value.trim().is_empty()) {
        let mut keys = load_session_keys(session_id).await?;
        if let Some(public_key) = provided_public_key.filter(|value| !value.trim().is_empty()) {
            keys.public_key = public_key.to_string();
        }
        if let Some(server_key) = provided_server_key.filter(|value| !value.trim().is_empty()) {
            keys.server_key = server_key.to_string();
        }
        return Ok(keys);
    }

    let public_key = provided_public_key
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| bad_request("Request needs public_key when no session_id is provided"))?;
    let server_key = provided_server_key
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| bad_request("Request needs server_key when no session_id is provided"))?;

    Ok(SessionKeys {
        public_key: public_key.to_string(),
        server_key: server_key.to_string(),
    })
}

async fn load_risk_patterns(pattern_id: Option<i32>) -> Result<Vec<RiskPattern>, ApiError> {
    let _timer = FunctionTimer::start("load_risk_patterns");
    initialize_database().await?;
    let client = connect_database().await?;

    let rows = if let Some(id) = pattern_id {
        client
            .query(
                "SELECT id, sequence FROM risk_patterns WHERE id = $1 ORDER BY id",
                &[&id],
            )
            .await
    } else {
        client
            .query("SELECT id, sequence FROM risk_patterns ORDER BY id", &[])
            .await
    }
    .map_err(|e| internal_error(format!("Failed to load risk patterns: {e}")))?;

    let patterns: Vec<RiskPattern> = rows
        .into_iter()
        .map(|row| RiskPattern {
            id: row.get("id"),
            sequence: row.get("sequence"),
        })
        .collect();

    if patterns.is_empty() {
        return Err(bad_request("No risk patterns found for selection"));
    }

    Ok(patterns)
}

pub fn encode_dna(seq: &str) -> Result<Vec<u8>, String> {
    seq.to_uppercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            'A' => Ok(0u8),
            'T' => Ok(1u8),
            'C' => Ok(2u8),
            'G' => Ok(3u8),
            other => Err(format!("Illegal base '{other}' in sequence")),
        })
        .collect()
}

pub fn parse_pattern(pattern_str: &str) -> Result<Vec<u8>, String> {
    let clean: String = pattern_str.chars().filter(|c| !c.is_whitespace()).collect();

    if clean.is_empty() {
        return Err("Risk pattern must not be empty".to_string());
    }

    if clean.chars().all(|c| c.is_ascii_digit()) {
        return clean
            .chars()
            .map(|c| {
                c.to_digit(10)
                    .ok_or_else(|| format!("Not a digit: '{c}'"))
                    .and_then(|d| {
                        if d <= 3 {
                            Ok(d as u8)
                        } else {
                            Err(format!("Digit {d} is not a valid DNA code (0-3)"))
                        }
                    })
            })
            .collect();
    }

    encode_dna(&clean)
}

fn b64_decode(encoded: &str, label: &str) -> Result<Vec<u8>, ApiError> {
    BASE64
        .decode(encoded)
        .map_err(|e| bad_request(format!("Invalid {label} base64: {e}")))
}

fn decode_public_key(encoded: &str) -> Result<CompactPublicKey, ApiError> {
    let _timer = FunctionTimer::start("decode_public_key");
    let bytes = b64_decode(encoded, "public_key")?;
    bincode::deserialize(&bytes)
        .map_err(|e| bad_request(format!("Failed to deserialize public_key: {e}")))
}

fn decode_server_key(encoded: &str) -> Result<ServerKey, ApiError> {
    let _timer = FunctionTimer::start("decode_server_key");
    let bytes = b64_decode(encoded, "server_key")?;
    let compressed: CompressedServerKey = bincode::deserialize(&bytes)
        .map_err(|e| bad_request(format!("Failed to deserialize server_key: {e}")))?;
    Ok(compressed.decompress())
}

fn with_server_key<F, R>(server_key: ServerKey, f: F) -> Result<R, ApiError>
where
    F: FnOnce() -> Result<R, ApiError> + Send,
    R: Send,
{
    let _timer = FunctionTimer::start("with_server_key");
    let _guard = FHE_SERVER_KEY_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| internal_error("FHE server-key lock is poisoned"))?;

    rayon::broadcast(|_| set_server_key(server_key.clone()));
    set_server_key(server_key);
    f()
}

fn encrypt_clear_values(
    values: &[u8],
    public_key: &CompactPublicKey,
) -> Result<Vec<FheUint8>, ApiError> {
    let _timer = FunctionTimer::start("encrypt_clear_values");
    encrypt_clear_values_on_current_pool(values, public_key)
}

fn encrypt_clear_values_on_current_pool(
    values: &[u8],
    public_key: &CompactPublicKey,
) -> Result<Vec<FheUint8>, ApiError> {
    let _timer = FunctionTimer::start("encrypt_clear_values_on_current_pool");
    if values.is_empty() {
        return Ok(Vec::new());
    }

    let mut builder = CompactCiphertextList::builder(public_key);
    for &value in values {
        builder.push(value);
    }

    let list = builder.build();
    let expander = list
        .expand()
        .map_err(|e| internal_error(format!("Failed to expand encrypted values: {e}")))?;

    (0..values.len())
        .map(|index| {
            expander
                .get::<FheUint8>(index)
                .map_err(|e| internal_error(format!("Failed to read encrypted value: {e}")))?
                .ok_or_else(|| internal_error(format!("Missing encrypted value at index {index}")))
        })
        .collect()
}

fn fhe_min(a: &FheUint8, b: &FheUint8) -> FheUint8 {
    let cond = a.lt(b);
    cond.if_then_else(a, b)
}

fn fhe_min3(a: &FheUint8, b: &FheUint8, c: &FheUint8) -> FheUint8 {
    let ab = fhe_min(a, b);
    fhe_min(&ab, c)
}

fn homomorphic_hamming_distance(window: &[FheUint8], pattern: &[u8]) -> FheUint8 {
    let mut pairs = window.iter().zip(pattern.iter());
    let (first_value, first_pattern) = pairs
        .next()
        .expect("hamming distance needs a non-empty pattern");
    let mut acc = FheUint8::cast_from(first_value.ne(*first_pattern));

    for (value, pattern_value) in pairs {
        let diff = FheUint8::cast_from(value.ne(*pattern_value));
        acc = &acc + &diff;
    }
    acc
}

fn homomorphic_hamming_distance_encrypted(
    window: &[FheUint8],
    enc_pattern: &[FheUint8],
) -> FheUint8 {
    let mut pairs = window.iter().zip(enc_pattern.iter());
    let (first_value, first_pattern) = pairs
        .next()
        .expect("hamming distance needs a non-empty pattern");
    let mut acc = FheUint8::cast_from(first_value.ne(first_pattern));

    for (value, pattern_value) in pairs {
        let diff = FheUint8::cast_from(value.ne(pattern_value));
        acc = &acc + &diff;
    }
    acc
}

pub fn homomorphic_sliding_window(
    enc_seq: &[FheUint8],
    pattern: &[u8],
) -> Result<Vec<FheUint8>, ApiError> {
    let _timer = FunctionTimer::start("homomorphic_sliding_window");
    let n = enc_seq.len();
    let m = pattern.len();

    if n == 0 {
        return Err(bad_request("Encrypted sequence must not be empty"));
    }
    if m == 0 {
        return Err(bad_request("Risk pattern must not be empty"));
    }
    if m > n {
        return Err(bad_request("Risk pattern is longer than sequence"));
    }
    if m > u8::MAX as usize {
        return Err(bad_request(
            "Risk pattern is too long for uint8 Hamming distances",
        ));
    }

    let windows = 0..=(n - m);
    if n - m + 1 >= PARALLEL_HAMMING_WINDOWS {
        Ok(windows
            .into_par_iter()
            .map(|start| {
                let window = &enc_seq[start..start + m];
                homomorphic_hamming_distance(window, pattern)
            })
            .collect())
    } else {
        Ok(windows
            .map(|start| {
                let window = &enc_seq[start..start + m];
                homomorphic_hamming_distance(window, pattern)
            })
            .collect())
    }
}

fn homomorphic_sliding_window_encrypted(
    enc_seq: &[FheUint8],
    enc_pattern: &[FheUint8],
) -> Result<Vec<FheUint8>, ApiError> {
    let _timer = FunctionTimer::start("homomorphic_sliding_window_encrypted");
    let n = enc_seq.len();
    let m = enc_pattern.len();

    if n == 0 {
        return Err(bad_request("Encrypted sequence must not be empty"));
    }
    if m == 0 {
        return Err(bad_request("Risk pattern must not be empty"));
    }
    if m > n {
        return Err(bad_request("Risk pattern is longer than sequence"));
    }
    if m > u8::MAX as usize {
        return Err(bad_request(
            "Risk pattern is too long for uint8 Hamming distances",
        ));
    }

    let windows = 0..=(n - m);
    if n - m + 1 >= PARALLEL_HAMMING_WINDOWS {
        Ok(windows
            .into_par_iter()
            .map(|start| {
                let window = &enc_seq[start..start + m];
                homomorphic_hamming_distance_encrypted(window, enc_pattern)
            })
            .collect())
    } else {
        Ok(windows
            .map(|start| {
                let window = &enc_seq[start..start + m];
                homomorphic_hamming_distance_encrypted(window, enc_pattern)
            })
            .collect())
    }
}

fn compare_against_encrypted_patterns(
    input_sequence: &[FheUint8],
    patterns: &[EncryptedRiskPattern],
) -> Result<Vec<Vec<FheUint8>>, ApiError> {
    let _timer = FunctionTimer::start("compare_against_encrypted_patterns");
    patterns
        .par_iter()
        .map(|pattern| {
            homomorphic_sliding_window_encrypted(input_sequence, &pattern.encrypted_bases)
        })
        .collect()
}

fn homomorphic_levenshtein_distance(
    seq_a: &[FheUint8],
    seq_b: &[u8],
) -> Result<FheUint8, ApiError> {
    let _timer = FunctionTimer::start("homomorphic_levenshtein_distance");
    let m = seq_a.len();
    let n = seq_b.len();

    if m == 0 || n == 0 {
        return Err(bad_request(
            "Levenshtein comparison needs two non-empty sequences",
        ));
    }
    if m > u8::MAX as usize || n > u8::MAX as usize {
        return Err(bad_request(
            "Sequences are too long for uint8 Levenshtein distances",
        ));
    }

    let init_values = encrypted_counting_values(m.max(n), &seq_a[0]);
    let zero = init_values[0].clone();
    let mut dp: Vec<Vec<FheUint8>> = vec![vec![zero.clone(); n + 1]; m + 1];

    for i in 0..=m {
        dp[i][0] = init_values[i].clone();
    }

    for j in 0..=n {
        dp[0][j] = init_values[j].clone();
    }

    for diagonal in 2..=(m + n) {
        let start_i = diagonal.saturating_sub(n).max(1);
        let end_i = (diagonal - 1).min(m);

        if start_i > end_i {
            continue;
        }

        let cells: Vec<usize> = (start_i..=end_i).collect();
        let values: Vec<(usize, FheUint8)> = if cells.len() >= PARALLEL_LEVENSHTEIN_CELLS {
            cells
                .par_iter()
                .map(|&i| {
                    let j = diagonal - i;
                    (i, levenshtein_cell(seq_a, seq_b, &dp, i, j))
                })
                .collect()
        } else {
            cells
                .iter()
                .map(|&i| {
                    let j = diagonal - i;
                    (i, levenshtein_cell(seq_a, seq_b, &dp, i, j))
                })
                .collect()
        };

        for (i, value) in values {
            let j = diagonal - i;
            dp[i][j] = value;
        }
    }

    Ok(dp[m][n].clone())
}

fn homomorphic_levenshtein_distance_encrypted(
    seq_a: &[FheUint8],
    seq_b: &[FheUint8],
) -> Result<FheUint8, ApiError> {
    let _timer = FunctionTimer::start("homomorphic_levenshtein_distance_encrypted");
    let m = seq_a.len();
    let n = seq_b.len();

    if m == 0 || n == 0 {
        return Err(bad_request(
            "Levenshtein comparison needs two non-empty sequences",
        ));
    }
    if m > u8::MAX as usize || n > u8::MAX as usize {
        return Err(bad_request(
            "Sequences are too long for uint8 Levenshtein distances",
        ));
    }

    let init_values = encrypted_counting_values(m.max(n), &seq_a[0]);
    let zero = init_values[0].clone();
    let mut dp: Vec<Vec<FheUint8>> = vec![vec![zero.clone(); n + 1]; m + 1];

    for i in 0..=m {
        dp[i][0] = init_values[i].clone();
    }

    for j in 0..=n {
        dp[0][j] = init_values[j].clone();
    }

    for diagonal in 2..=(m + n) {
        let start_i = diagonal.saturating_sub(n).max(1);
        let end_i = (diagonal - 1).min(m);

        if start_i > end_i {
            continue;
        }

        let cells: Vec<usize> = (start_i..=end_i).collect();
        let values: Vec<(usize, FheUint8)> = if cells.len() >= PARALLEL_LEVENSHTEIN_CELLS {
            cells
                .par_iter()
                .map(|&i| {
                    let j = diagonal - i;
                    (i, levenshtein_cell_encrypted(seq_a, seq_b, &dp, i, j))
                })
                .collect()
        } else {
            cells
                .iter()
                .map(|&i| {
                    let j = diagonal - i;
                    (i, levenshtein_cell_encrypted(seq_a, seq_b, &dp, i, j))
                })
                .collect()
        };

        for (i, value) in values {
            let j = diagonal - i;
            dp[i][j] = value;
        }
    }

    Ok(dp[m][n].clone())
}

fn encrypted_counting_values(max_value: usize, reference: &FheUint8) -> Vec<FheUint8> {
    let zero = FheUint8::cast_from(reference.ne(reference));
    let mut values = Vec::with_capacity(max_value + 1);
    values.push(zero);

    for value in 1..=max_value {
        values.push(&values[value - 1] + 1u8);
    }

    values
}

fn levenshtein_cell(
    seq_a: &[FheUint8],
    seq_b: &[u8],
    dp: &[Vec<FheUint8>],
    i: usize,
    j: usize,
) -> FheUint8 {
    let cost = FheUint8::cast_from(seq_a[i - 1].ne(seq_b[j - 1]));
    let deletion = &dp[i - 1][j] + 1u8;
    let insertion = &dp[i][j - 1] + 1u8;
    let substitution = &dp[i - 1][j - 1] + cost;

    fhe_min3(&deletion, &insertion, &substitution)
}

fn levenshtein_cell_encrypted(
    seq_a: &[FheUint8],
    seq_b: &[FheUint8],
    dp: &[Vec<FheUint8>],
    i: usize,
    j: usize,
) -> FheUint8 {
    let cost = FheUint8::cast_from(seq_a[i - 1].ne(&seq_b[j - 1]));
    let deletion = &dp[i - 1][j] + 1u8;
    let insertion = &dp[i][j - 1] + 1u8;
    let substitution = &dp[i - 1][j - 1] + cost;

    fhe_min3(&deletion, &insertion, &substitution)
}

fn compare_against_encrypted_patterns_levenshtein(
    input_sequence: &[FheUint8],
    patterns: &[EncryptedRiskPattern],
) -> Result<Vec<FheUint8>, ApiError> {
    let _timer = FunctionTimer::start("compare_against_encrypted_patterns_levenshtein");
    patterns
        .par_iter()
        .map(|pattern| {
            homomorphic_levenshtein_distance_encrypted(input_sequence, &pattern.encrypted_bases)
        })
        .collect()
}

pub fn deserialize_fhe_vec(bytes: &[u8]) -> Result<Vec<FheUint8>, ApiError> {
    let _timer = FunctionTimer::start("deserialize_fhe_vec");
    bincode::deserialize(bytes)
        .map_err(|e| bad_request(format!("Failed to deserialize encrypted vector: {e}")))
}

fn serialize_fhe_item(data: &FheUint8) -> Result<String, ApiError> {
    bincode::serialize(data)
        .map(|bytes| BASE64.encode(bytes))
        .map_err(|e| internal_error(format!("Failed to serialize encrypted item: {e}")))
}

fn deserialize_fhe_item(encoded: &str) -> Result<FheUint8, ApiError> {
    let bytes = b64_decode(encoded, "encrypted item")?;
    bincode::deserialize(&bytes)
        .map_err(|e| bad_request(format!("Failed to deserialize encrypted item: {e}")))
}

fn serialize_fhe_items(data: &[FheUint8]) -> Result<Vec<String>, ApiError> {
    let _timer = FunctionTimer::start("serialize_fhe_items");
    data.par_iter().map(serialize_fhe_item).collect()
}

fn deserialize_encrypted_sequence(
    encrypted_sequence: Option<&str>,
    encrypted_bases: Option<&[String]>,
) -> Result<Vec<FheUint8>, ApiError> {
    let _timer = FunctionTimer::start("deserialize_encrypted_sequence");
    if let Some(items) = encrypted_bases.filter(|items| !items.is_empty()) {
        return items
            .par_iter()
            .map(|item| deserialize_fhe_item(item))
            .collect();
    }

    if let Some(blob) = encrypted_sequence.filter(|value| !value.trim().is_empty()) {
        let enc_bytes = b64_decode(blob, "encrypted_sequence")?;
        let values = deserialize_fhe_vec(&enc_bytes)?;
        if values.is_empty() {
            return Err(bad_request("Encrypted sequence must not be empty"));
        }
        return Ok(values);
    }

    Err(bad_request(
        "Request must include encrypted_bases or encrypted_sequence",
    ))
}

fn encrypt_risk_patterns(
    patterns: &[RiskPattern],
    public_key: &CompactPublicKey,
) -> Result<Vec<EncryptedRiskPattern>, ApiError> {
    let _timer = FunctionTimer::start("encrypt_risk_patterns");
    patterns
        .iter()
        .map(|pattern| {
            let encoded = encode_dna(&pattern.sequence).map_err(bad_request)?;
            if encoded.is_empty() {
                return Err(bad_request(format!(
                    "Risk pattern {} must not be empty",
                    pattern.id
                )));
            }

            Ok(EncryptedRiskPattern {
                pattern: pattern.clone(),
                encrypted_bases: encrypt_clear_values_on_current_pool(&encoded, public_key)?,
            })
        })
        .collect()
}

// API DTOs
#[derive(Deserialize, Serialize, JsonSchema)]
pub struct EncryptRequest {
    pub sequence: String,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct EncryptResponse {
    pub encrypted_bases: Vec<String>,
    pub original_length: usize,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct ProcessRequest {
    #[serde(default)]
    pub encrypted_sequence: Option<String>,
    #[serde(default)]
    pub encrypted_bases: Option<Vec<String>>,
    #[serde(default)]
    pub server_key: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct ProcessResponse {
    pub encrypted_distance_items: Vec<String>,
    pub windows: usize,
}

#[derive(Serialize, JsonSchema)]
pub struct PatternsResponse {
    pub patterns: Vec<RiskPattern>,
}

#[derive(Serialize, JsonSchema)]
pub struct SessionsResponse {
    pub sessions: Vec<GenomicsSession>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct CreateSessionRequest {
    pub public_key: String,
    pub server_key: String,
}

#[derive(Serialize, JsonSchema)]
pub struct CreateSessionResponse {
    pub session: GenomicsSession,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct CompareDatabaseRequest {
    #[serde(default)]
    pub encrypted_sequence: Option<String>,
    #[serde(default)]
    pub encrypted_bases: Option<Vec<String>>,
    #[serde(default)]
    pub server_key: Option<String>,
    #[serde(default)]
    pub public_key: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub pattern_id: Option<i32>,
}

#[derive(Serialize, JsonSchema)]
pub struct CompareDatabaseResponse {
    pub encrypted_result_items: Vec<Vec<String>>,
    pub compared_sequences: usize,
    pub patterns: Vec<RiskPattern>,
}

#[derive(Serialize, JsonSchema)]
pub struct ProcessLevenshteinResponse {
    pub encrypted_distance_items: Vec<String>,
    pub windows: usize,
}

#[derive(Serialize, JsonSchema)]
pub struct CompareDatabaseLevenshteinResponse {
    pub encrypted_result_items: Vec<Vec<String>>,
    pub compared_sequences: usize,
    pub patterns: Vec<RiskPattern>,
}

fn encrypt_sequence(
    req: EncryptRequest,
    public_key_b64: String,
) -> Result<EncryptResponse, ApiError> {
    let _timer = FunctionTimer::start("encrypt_sequence");
    let clean = encode_dna(&req.sequence).map_err(bad_request)?;
    if clean.is_empty() {
        return Err(bad_request("Sequence must not be empty"));
    }

    let public_key = decode_public_key(&public_key_b64)?;
    let len = clean.len();

    let encrypted = encrypt_clear_values(&clean, &public_key)?;

    Ok(EncryptResponse {
        encrypted_bases: serialize_fhe_items(&encrypted)?,
        original_length: len,
    })
}

fn process_hamming(req: ProcessRequest, keys: SessionKeys) -> Result<ProcessResponse, ApiError> {
    let _timer = FunctionTimer::start("process_hamming");
    let enc_seq = deserialize_encrypted_sequence(
        req.encrypted_sequence.as_deref(),
        req.encrypted_bases.as_deref(),
    )?;
    let pattern = parse_pattern(SERVER_RISK_PATTERN).map_err(bad_request)?;
    let server_key = decode_server_key(&keys.server_key)?;

    let enc_distances = with_server_key(server_key, move || {
        homomorphic_sliding_window(&enc_seq, &pattern)
    })?;

    let windows = enc_distances.len();
    Ok(ProcessResponse {
        encrypted_distance_items: serialize_fhe_items(&enc_distances)?,
        windows,
    })
}

fn compare_database(
    req: CompareDatabaseRequest,
    patterns: Vec<RiskPattern>,
    keys: SessionKeys,
) -> Result<CompareDatabaseResponse, ApiError> {
    let _timer = FunctionTimer::start("compare_database");
    let enc_input = deserialize_encrypted_sequence(
        req.encrypted_sequence.as_deref(),
        req.encrypted_bases.as_deref(),
    )?;
    let public_key = decode_public_key(&keys.public_key)?;
    let server_key = decode_server_key(&keys.server_key)?;
    let encrypted_patterns = encrypt_risk_patterns(&patterns, &public_key)?;
    let response_patterns: Vec<RiskPattern> = encrypted_patterns
        .iter()
        .map(|pattern| pattern.pattern.clone())
        .collect();

    let results = with_server_key(server_key, move || {
        compare_against_encrypted_patterns(&enc_input, &encrypted_patterns)
    })?;

    let encrypted_result_items: Vec<Vec<String>> = results
        .par_iter()
        .map(|distances| serialize_fhe_items(distances))
        .collect::<Result<_, _>>()?;

    Ok(CompareDatabaseResponse {
        encrypted_result_items,
        compared_sequences: response_patterns.len(),
        patterns: response_patterns,
    })
}

fn process_levenshtein(
    req: ProcessRequest,
    keys: SessionKeys,
) -> Result<ProcessLevenshteinResponse, ApiError> {
    let _timer = FunctionTimer::start("process_levenshtein");
    let enc_seq = deserialize_encrypted_sequence(
        req.encrypted_sequence.as_deref(),
        req.encrypted_bases.as_deref(),
    )?;
    let pattern = parse_pattern(SERVER_RISK_PATTERN).map_err(bad_request)?;
    let server_key = decode_server_key(&keys.server_key)?;

    let distance = with_server_key(server_key, move || {
        homomorphic_levenshtein_distance(&enc_seq, &pattern)
    })?;

    let distances = vec![distance];
    Ok(ProcessLevenshteinResponse {
        encrypted_distance_items: serialize_fhe_items(&distances)?,
        windows: 1,
    })
}

fn compare_database_levenshtein(
    req: CompareDatabaseRequest,
    patterns: Vec<RiskPattern>,
    keys: SessionKeys,
) -> Result<CompareDatabaseLevenshteinResponse, ApiError> {
    let _timer = FunctionTimer::start("compare_database_levenshtein");
    let enc_input = deserialize_encrypted_sequence(
        req.encrypted_sequence.as_deref(),
        req.encrypted_bases.as_deref(),
    )?;
    let public_key = decode_public_key(&keys.public_key)?;
    let server_key = decode_server_key(&keys.server_key)?;
    let encrypted_patterns = encrypt_risk_patterns(&patterns, &public_key)?;
    let response_patterns: Vec<RiskPattern> = encrypted_patterns
        .iter()
        .map(|pattern| pattern.pattern.clone())
        .collect();

    let results = with_server_key(server_key, move || {
        compare_against_encrypted_patterns_levenshtein(&enc_input, &encrypted_patterns)
    })?;

    let encrypted_result_items: Vec<Vec<String>> = results
        .par_iter()
        .map(|distance| serialize_fhe_items(&[distance.clone()]))
        .collect::<Result<_, _>>()?;

    Ok(CompareDatabaseLevenshteinResponse {
        encrypted_result_items,
        compared_sequences: response_patterns.len(),
        patterns: response_patterns,
    })
}

// API Handler
pub async fn sessions_handler() -> Result<axum::Json<SessionsResponse>, ApiError> {
    let _timer = FunctionTimer::start("sessions_handler");
    let sessions = list_sessions().await?;
    Ok(axum::Json(SessionsResponse { sessions }))
}

pub async fn create_session_handler(
    axum::Json(req): axum::Json<CreateSessionRequest>,
) -> Result<axum::Json<CreateSessionResponse>, ApiError> {
    let _timer = FunctionTimer::start("create_session_handler");
    let session = create_session(req.public_key, req.server_key).await?;
    Ok(axum::Json(CreateSessionResponse { session }))
}

pub async fn patterns_handler() -> Result<axum::Json<PatternsResponse>, ApiError> {
    let _timer = FunctionTimer::start("patterns_handler");
    let patterns = load_risk_patterns(None).await?;
    Ok(axum::Json(PatternsResponse { patterns }))
}

pub async fn encrypt_handler(
    axum::Json(req): axum::Json<EncryptRequest>,
) -> Result<axum::Json<EncryptResponse>, ApiError> {
    let _timer = FunctionTimer::start("encrypt_handler");
    let public_key =
        resolve_public_key(req.session_id.as_deref(), req.public_key.as_deref()).await?;
    let response = tokio::task::spawn_blocking(move || encrypt_sequence(req, public_key))
        .await
        .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn process_handler(
    axum::Json(req): axum::Json<ProcessRequest>,
) -> Result<axum::Json<ProcessResponse>, ApiError> {
    let _timer = FunctionTimer::start("process_handler");
    let keys = resolve_session_keys(
        req.session_id.as_deref(),
        req.public_key.as_deref(),
        req.server_key.as_deref(),
    )
    .await?;
    let response = tokio::task::spawn_blocking(move || process_hamming(req, keys))
        .await
        .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn compare_database_handler(
    axum::Json(req): axum::Json<CompareDatabaseRequest>,
) -> Result<axum::Json<CompareDatabaseResponse>, ApiError> {
    let _timer = FunctionTimer::start("compare_database_handler");
    let keys = resolve_session_keys(
        req.session_id.as_deref(),
        req.public_key.as_deref(),
        req.server_key.as_deref(),
    )
    .await?;
    let patterns = load_risk_patterns(req.pattern_id).await?;
    let response = tokio::task::spawn_blocking(move || compare_database(req, patterns, keys))
        .await
        .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn process_levenshtein_handler(
    axum::Json(req): axum::Json<ProcessRequest>,
) -> Result<axum::Json<ProcessLevenshteinResponse>, ApiError> {
    let _timer = FunctionTimer::start("process_levenshtein_handler");
    let keys = resolve_session_keys(
        req.session_id.as_deref(),
        req.public_key.as_deref(),
        req.server_key.as_deref(),
    )
    .await?;
    let response = tokio::task::spawn_blocking(move || process_levenshtein(req, keys))
        .await
        .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn compare_database_levenshtein_handler(
    axum::Json(req): axum::Json<CompareDatabaseRequest>,
) -> Result<axum::Json<CompareDatabaseLevenshteinResponse>, ApiError> {
    let _timer = FunctionTimer::start("compare_database_levenshtein_handler");
    let keys = resolve_session_keys(
        req.session_id.as_deref(),
        req.public_key.as_deref(),
        req.server_key.as_deref(),
    )
    .await?;
    let patterns = load_risk_patterns(req.pattern_id).await?;
    let response =
        tokio::task::spawn_blocking(move || compare_database_levenshtein(req, patterns, keys))
            .await
            .map_err(join_error)??;
    Ok(axum::Json(response))
}
