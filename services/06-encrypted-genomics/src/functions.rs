use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rayon::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Instant;
use tfhe::prelude::*;
use tfhe::{
    set_server_key, CompactCiphertextList, CompactPublicKey, CompressedServerKey, FheUint8,
    ServerKey,
};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore};
use tokio_postgres::NoTls;

type ApiError = (axum::http::StatusCode, String);
type IndexedRiskPattern = (usize, RiskPattern);

const DEFAULT_DATABASE_CONFIG: &str =
    "host=159.195.145.100 port=5432 user=genomics password=genomics dbname=genomics";
const PARALLEL_HAMMING_WINDOWS: usize = 2;
const PARALLEL_LEVENSHTEIN_CELLS: usize = 2;
const PROCESSING_QUEUE_CAPACITY: usize = 5;
const MAX_HAMMING_SEQUENCE_LEN: usize = 255;
const MAX_LEVENSHTEIN_SEQUENCE_LEN: usize = 122;
const KEY_CACHE_CAPACITY: usize = 8;
const PARALLEL_HAMMING_PATTERN_CELLS: usize = 32;

static FHE_SERVER_KEY_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static PROCESSING_QUEUE: OnceLock<ProcessingQueue> = OnceLock::new();
static DATABASE_INITIALIZED: AtomicBool = AtomicBool::new(false);
static PUBLIC_KEY_CACHE: OnceLock<Mutex<DecodedKeyCache<CompactPublicKey>>> = OnceLock::new();
static SERVER_KEY_CACHE: OnceLock<Mutex<DecodedKeyCache<ServerKey>>> = OnceLock::new();

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProcessingJobState {
    Queued,
    Running,
}

impl ProcessingJobState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
        }
    }
}

#[derive(Clone)]
struct ProcessingQueueEntry {
    id: u64,
    session_id: String,
    job_type: String,
    state: ProcessingJobState,
}

struct ProcessingQueue {
    entries: Mutex<VecDeque<ProcessingQueueEntry>>,
    notifier: Notify,
    capacity: Arc<Semaphore>,
    next_id: AtomicU64,
}

struct DecodedKeyCache<K> {
    entries: HashMap<String, K>,
    order: VecDeque<String>,
}

impl<K: Clone> DecodedKeyCache<K> {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&self, encoded: &str) -> Option<K> {
        self.entries.get(encoded).cloned()
    }

    fn insert(&mut self, encoded: String, key: K) {
        if !self.entries.contains_key(&encoded) {
            self.order.push_back(encoded.clone());
        }
        self.entries.insert(encoded, key);

        while self.order.len() > KEY_CACHE_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

impl ProcessingQueue {
    fn new() -> Self {
        Self {
            entries: Mutex::new(VecDeque::new()),
            notifier: Notify::new(),
            capacity: Arc::new(Semaphore::new(PROCESSING_QUEUE_CAPACITY)),
            next_id: AtomicU64::new(1),
        }
    }

    fn snapshot(&self) -> Result<FifoStatusResponse, ApiError> {
        let entries = self
            .entries
            .lock()
            .map_err(|_| internal_error("Processing FIFO lock is poisoned"))?;
        let jobs = entries
            .iter()
            .enumerate()
            .map(|(index, entry)| FifoJobStatus {
                id: entry.id,
                position: index + 1,
                session_id: entry.session_id.clone(),
                job_type: entry.job_type.clone(),
                state: entry.state.as_str().to_string(),
            })
            .collect::<Vec<_>>();
        let used = jobs.len();

        Ok(FifoStatusResponse {
            capacity: PROCESSING_QUEUE_CAPACITY,
            used,
            locked: used >= PROCESSING_QUEUE_CAPACITY,
            jobs,
        })
    }
}

struct QueuedProcessingJob {
    queue: &'static ProcessingQueue,
    id: u64,
    _permit: OwnedSemaphorePermit,
}

impl QueuedProcessingJob {
    async fn wait_turn(&self) -> Result<(), ApiError> {
        let _timer = FunctionTimer::start("wait_fifo_turn");
        loop {
            let notified = self.queue.notifier.notified();
            {
                let mut entries = self
                    .queue
                    .entries
                    .lock()
                    .map_err(|_| internal_error("Processing FIFO lock is poisoned"))?;

                if let Some(front) = entries.front_mut() {
                    if front.id == self.id {
                        front.state = ProcessingJobState::Running;
                        return Ok(());
                    }
                }

                if !entries.iter().any(|entry| entry.id == self.id) {
                    return Err(internal_error("Processing FIFO job disappeared"));
                }
            }

            notified.await;
        }
    }
}

impl Drop for QueuedProcessingJob {
    fn drop(&mut self) {
        if let Ok(mut entries) = self.queue.entries.lock() {
            if let Some(index) = entries.iter().position(|entry| entry.id == self.id) {
                entries.remove(index);
            }
        }
        self.queue.notifier.notify_waiters();
    }
}

#[derive(Clone)]
struct EncryptedRiskPattern {
    encrypted_bases: Vec<FheUint8>,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SkippedComparison {
    pub index: usize,
    pub reason: String,
}

#[derive(Clone)]
struct StoredSessionSequence {
    info: SessionSequenceInfo,
    encrypted_bases: Vec<String>,
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

fn too_many_requests(message: impl Into<String>) -> ApiError {
    (axum::http::StatusCode::TOO_MANY_REQUESTS, message.into())
}

fn join_error(error: tokio::task::JoinError) -> ApiError {
    internal_error(format!("Blocking task failed: {error}"))
}

async fn ensure_database_initialized() -> Result<(), ApiError> {
    if DATABASE_INITIALIZED.load(Ordering::Acquire) {
        return Ok(());
    }

    initialize_database().await
}

fn processing_queue() -> &'static ProcessingQueue {
    PROCESSING_QUEUE.get_or_init(ProcessingQueue::new)
}

async fn enqueue_processing_job(
    session_id: Option<&str>,
    job_type: &'static str,
) -> Result<QueuedProcessingJob, ApiError> {
    let _timer = FunctionTimer::start("enqueue_processing_job");
    let queue = processing_queue();
    let permit = queue
        .capacity
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            too_many_requests(format!(
                "FIFO ist voll. Maximal {PROCESSING_QUEUE_CAPACITY} Auftraege koennen gleichzeitig in der Liste stehen."
            ))
        })?;
    let id = queue.next_id.fetch_add(1, Ordering::Relaxed);
    let session_id = session_id
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("direct")
        .to_string();

    {
        let mut entries = queue
            .entries
            .lock()
            .map_err(|_| internal_error("Processing FIFO lock is poisoned"))?;
        entries.push_back(ProcessingQueueEntry {
            id,
            session_id,
            job_type: job_type.to_string(),
            state: ProcessingJobState::Queued,
        });
    }

    queue.notifier.notify_waiters();
    Ok(QueuedProcessingJob {
        queue,
        id,
        _permit: permit,
    })
}

fn database_config() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| DEFAULT_DATABASE_CONFIG.to_string())
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

            CREATE TABLE IF NOT EXISTS sessionsequences (
                id SERIAL PRIMARY KEY,
                session_id TEXT NOT NULL REFERENCES genomics_sessions(id) ON DELETE CASCADE,
                public_key TEXT NOT NULL,
                encrypted_bases TEXT NOT NULL,
                original_length INTEGER NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT now()
            );

            CREATE INDEX IF NOT EXISTS sessionsequences_session_id_idx
            ON sessionsequences (session_id);
            ",
        )
        .await
        .map_err(|e| internal_error(format!("Failed to create risk_patterns table: {e}")))?;

    DATABASE_INITIALIZED.store(true, Ordering::Release);
    Ok(())
}

async fn create_session(
    public_key: String,
    server_key: String,
) -> Result<GenomicsSession, ApiError> {
    let _timer = FunctionTimer::start("create_session");
    ensure_database_initialized().await?;
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
    ensure_database_initialized().await?;
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
    ensure_database_initialized().await?;
    let client = connect_database().await?;
    load_session_keys_from_client(&client, session_id).await
}

async fn load_session_keys_from_client(
    client: &tokio_postgres::Client,
    session_id: &str,
) -> Result<SessionKeys, ApiError> {
    let _timer = FunctionTimer::start("load_session_keys_from_client");
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
    ensure_database_initialized().await?;
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

    Ok(patterns)
}

async fn load_single_risk_pattern(pattern_id: Option<i32>) -> Result<RiskPattern, ApiError> {
    let _timer = FunctionTimer::start("load_single_risk_pattern");
    load_risk_patterns(pattern_id)
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| bad_request("No risk patterns found for selection"))
}

async fn create_risk_pattern(sequence: String) -> Result<RiskPattern, ApiError> {
    let _timer = FunctionTimer::start("create_risk_pattern");
    let clean = normalize_dna_sequence(&sequence)?;
    ensure_database_initialized().await?;
    let client = connect_database().await?;

    let row = client
        .query_one(
            "INSERT INTO risk_patterns (sequence) VALUES ($1) \
             ON CONFLICT (sequence) DO UPDATE SET sequence = EXCLUDED.sequence \
             RETURNING id, sequence",
            &[&clean],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to create risk pattern: {e}")))?;

    Ok(RiskPattern {
        id: row.get("id"),
        sequence: row.get("sequence"),
    })
}

async fn list_session_sequence_groups() -> Result<Vec<SessionSequenceGroup>, ApiError> {
    let _timer = FunctionTimer::start("list_session_sequence_groups");
    ensure_database_initialized().await?;
    let client = connect_database().await?;

    let rows = client
        .query(
            "SELECT session_id, MAX(public_key) AS public_key, COUNT(*)::BIGINT AS sequence_count, \
             MAX(created_at)::TEXT AS latest_created_at \
             FROM sessionsequences \
             GROUP BY session_id \
             ORDER BY MAX(created_at) DESC",
            &[],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to list session sequences: {e}")))?;

    Ok(rows
        .into_iter()
        .map(|row| SessionSequenceGroup {
            session_id: row.get("session_id"),
            public_key: row.get("public_key"),
            sequence_count: row.get("sequence_count"),
            latest_created_at: row.get("latest_created_at"),
        })
        .collect())
}

async fn store_session_sequence(
    session_id: String,
    encrypted_bases: Vec<String>,
) -> Result<SessionSequenceInfo, ApiError> {
    let _timer = FunctionTimer::start("store_session_sequence");
    if encrypted_bases.is_empty() {
        return Err(bad_request("Encrypted sequence must not be empty"));
    }
    enforce_sequence_limit(
        encrypted_bases.len(),
        MAX_HAMMING_SEQUENCE_LEN,
        "Session sequence",
    )?;

    ensure_database_initialized().await?;
    let client = connect_database().await?;
    let keys = load_session_keys_from_client(&client, &session_id).await?;
    let encoded = serde_json::to_string(&encrypted_bases)
        .map_err(|e| internal_error(format!("Failed to serialize session sequence: {e}")))?;
    let original_length = encrypted_bases.len() as i32;

    let row = client
        .query_one(
            "INSERT INTO sessionsequences \
             (session_id, public_key, encrypted_bases, original_length) \
             VALUES ($1, $2, $3, $4) \
             RETURNING id, session_id, original_length, created_at::TEXT AS created_at",
            &[&session_id, &keys.public_key, &encoded, &original_length],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to store session sequence: {e}")))?;

    Ok(SessionSequenceInfo {
        id: row.get("id"),
        session_id: row.get("session_id"),
        original_length: row.get::<_, i32>("original_length") as usize,
        created_at: row.get("created_at"),
        encrypted_bases: Vec::new(),
    })
}

async fn load_session_sequences(session_id: &str) -> Result<Vec<StoredSessionSequence>, ApiError> {
    let _timer = FunctionTimer::start("load_session_sequences");
    ensure_database_initialized().await?;
    let client = connect_database().await?;
    let rows = client
        .query(
            "SELECT id, session_id, encrypted_bases, original_length, created_at::TEXT AS created_at \
             FROM sessionsequences \
             WHERE session_id = $1 \
             ORDER BY created_at DESC, id DESC",
            &[&session_id],
        )
        .await
        .map_err(|e| internal_error(format!("Failed to load session sequences: {e}")))?;

    let sequences = rows
        .into_iter()
        .map(|row| {
            let encrypted_json: String = row.get("encrypted_bases");
            let encrypted_bases: Vec<String> =
                serde_json::from_str(&encrypted_json).map_err(|e| {
                    internal_error(format!("Failed to deserialize session sequence: {e}"))
                })?;
            Ok(StoredSessionSequence {
                info: SessionSequenceInfo {
                    id: row.get("id"),
                    session_id: row.get("session_id"),
                    original_length: row.get::<_, i32>("original_length") as usize,
                    created_at: row.get("created_at"),
                    encrypted_bases: encrypted_bases.clone(),
                },
                encrypted_bases,
            })
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    if sequences.is_empty() {
        return Err(bad_request("No session sequences found for selection"));
    }

    Ok(sequences)
}

fn normalize_dna_sequence(sequence: &str) -> Result<String, ApiError> {
    let _timer = FunctionTimer::start("normalize_dna_sequence");
    let clean: String = sequence
        .to_uppercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    if clean.is_empty() {
        return Err(bad_request("Risk pattern must not be empty"));
    }

    if let Some(illegal) = clean
        .chars()
        .find(|base| !matches!(base, 'A' | 'T' | 'C' | 'G'))
    {
        return Err(bad_request(format!(
            "Illegal base '{illegal}' in risk pattern"
        )));
    }

    Ok(clean)
}

fn enforce_sequence_limit(len: usize, max_len: usize, operation: &str) -> Result<(), ApiError> {
    if len > max_len {
        return Err(bad_request(format!(
            "{operation} sequence is too long. Maximum is {max_len} bases."
        )));
    }

    Ok(())
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

fn decode_public_key_uncached(encoded: &str) -> Result<CompactPublicKey, ApiError> {
    let _timer = FunctionTimer::start("decode_public_key_uncached");
    let bytes = b64_decode(encoded, "public_key")?;
    bincode::deserialize(&bytes)
        .map_err(|e| bad_request(format!("Failed to deserialize public_key: {e}")))
}

fn decode_server_key_uncached(encoded: &str) -> Result<ServerKey, ApiError> {
    let _timer = FunctionTimer::start("decode_server_key_uncached");
    let bytes = b64_decode(encoded, "server_key")?;
    let compressed: CompressedServerKey = bincode::deserialize(&bytes)
        .map_err(|e| bad_request(format!("Failed to deserialize server_key: {e}")))?;
    Ok(compressed.decompress())
}

fn decode_public_key(encoded: &str) -> Result<CompactPublicKey, ApiError> {
    let _timer = FunctionTimer::start("decode_public_key");
    let cache = PUBLIC_KEY_CACHE.get_or_init(|| Mutex::new(DecodedKeyCache::new()));
    if let Some(key) = cache
        .lock()
        .map_err(|_| internal_error("Public-key cache lock is poisoned"))?
        .get(encoded)
    {
        return Ok(key);
    }

    let key = decode_public_key_uncached(encoded)?;
    cache
        .lock()
        .map_err(|_| internal_error("Public-key cache lock is poisoned"))?
        .insert(encoded.to_string(), key.clone());
    Ok(key)
}

fn decode_server_key(encoded: &str) -> Result<ServerKey, ApiError> {
    let _timer = FunctionTimer::start("decode_server_key");
    let cache = SERVER_KEY_CACHE.get_or_init(|| Mutex::new(DecodedKeyCache::new()));
    if let Some(key) = cache
        .lock()
        .map_err(|_| internal_error("Server-key cache lock is poisoned"))?
        .get(encoded)
    {
        return Ok(key);
    }

    let key = decode_server_key_uncached(encoded)?;
    cache
        .lock()
        .map_err(|_| internal_error("Server-key cache lock is poisoned"))?
        .insert(encoded.to_string(), key.clone());
    Ok(key)
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

fn homomorphic_hamming_distance_serial(window: &[FheUint8], pattern: &[u8]) -> FheUint8 {
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

fn homomorphic_hamming_distance(window: &[FheUint8], pattern: &[u8]) -> FheUint8 {
    if window.len() >= PARALLEL_HAMMING_PATTERN_CELLS && rayon::current_num_threads() > 1 {
        return window
            .par_iter()
            .zip(pattern.par_iter())
            .map(|(value, pattern_value)| FheUint8::cast_from(value.ne(*pattern_value)))
            .reduce_with(|left, right| &left + &right)
            .expect("hamming distance needs a non-empty pattern");
    }

    homomorphic_hamming_distance_serial(window, pattern)
}

fn homomorphic_hamming_distance_encrypted_serial(
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

fn homomorphic_hamming_distance_encrypted(
    window: &[FheUint8],
    enc_pattern: &[FheUint8],
) -> FheUint8 {
    if window.len() >= PARALLEL_HAMMING_PATTERN_CELLS && rayon::current_num_threads() > 1 {
        return window
            .par_iter()
            .zip(enc_pattern.par_iter())
            .map(|(value, pattern_value)| FheUint8::cast_from(value.ne(pattern_value)))
            .reduce_with(|left, right| &left + &right)
            .expect("hamming distance needs a non-empty pattern");
    }

    homomorphic_hamming_distance_encrypted_serial(window, enc_pattern)
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
                homomorphic_hamming_distance_serial(window, pattern)
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
                homomorphic_hamming_distance_encrypted_serial(window, enc_pattern)
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

fn prepare_risk_patterns_for_all_query(
    patterns: &[RiskPattern],
    input_len: usize,
    skip_too_long_patterns: bool,
) -> Result<(Vec<IndexedRiskPattern>, Vec<SkippedComparison>), ApiError> {
    let mut comparable_patterns = Vec::new();
    let mut skipped_results = Vec::new();

    for (index, pattern) in patterns.iter().enumerate() {
        let encoded = encode_dna(&pattern.sequence).map_err(bad_request)?;
        if skip_too_long_patterns && encoded.len() > input_len {
            skipped_results.push(SkippedComparison {
                index,
                reason: format!(
                    "Pattern longer than sequence ({} > {})",
                    encoded.len(),
                    input_len
                ),
            });
        } else {
            comparable_patterns.push((index, pattern.clone()));
        }
    }

    Ok((comparable_patterns, skipped_results))
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

    dp[0][..(n + 1)].clone_from_slice(&init_values[..(n + 1)]);

    for diagonal in 2..=(m + n) {
        let start_i = diagonal.saturating_sub(n).max(1);
        let end_i = (diagonal - 1).min(m);

        if start_i > end_i {
            continue;
        }

        let diagonal_len = end_i - start_i + 1;
        let values: Vec<(usize, FheUint8)> =
            if diagonal_len >= PARALLEL_LEVENSHTEIN_CELLS && rayon::current_num_threads() > 1 {
                (start_i..=end_i)
                    .into_par_iter()
                    .map(|i| {
                        let j = diagonal - i;
                        (i, levenshtein_cell(seq_a, seq_b, &dp, i, j))
                    })
                    .collect()
            } else {
                (start_i..=end_i)
                    .map(|i| {
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

    dp[0][..(n + 1)].clone_from_slice(&init_values[..(n + 1)]);

    for diagonal in 2..=(m + n) {
        let start_i = diagonal.saturating_sub(n).max(1);
        let end_i = (diagonal - 1).min(m);

        if start_i > end_i {
            continue;
        }

        let diagonal_len = end_i - start_i + 1;
        let values: Vec<(usize, FheUint8)> =
            if diagonal_len >= PARALLEL_LEVENSHTEIN_CELLS && rayon::current_num_threads() > 1 {
                (start_i..=end_i)
                    .into_par_iter()
                    .map(|i| {
                        let j = diagonal - i;
                        (i, levenshtein_cell_encrypted(seq_a, seq_b, &dp, i, j))
                    })
                    .collect()
            } else {
                (start_i..=end_i)
                    .map(|i| {
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

fn encrypted_counting_values(max_value: usize, _reference: &FheUint8) -> Vec<FheUint8> {
    (0..=max_value)
        .into_par_iter()
        .map(|value| FheUint8::encrypt_trivial(value as u8))
        .collect()
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
        .par_iter()
        .map(|pattern| {
            let encoded = encode_dna(&pattern.sequence).map_err(bad_request)?;
            if encoded.is_empty() {
                return Err(bad_request(format!(
                    "Risk pattern {} must not be empty",
                    pattern.id
                )));
            }

            Ok(EncryptedRiskPattern {
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
    #[serde(default)]
    pub pattern_id: Option<i32>,
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

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct CreateRiskPatternRequest {
    pub sequence: String,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct CreateRiskPatternResponse {
    pub pattern: RiskPattern,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SessionSequenceGroup {
    pub session_id: String,
    pub public_key: String,
    pub sequence_count: i64,
    pub latest_created_at: String,
}

#[derive(Clone, Debug, Serialize, JsonSchema)]
pub struct SessionSequenceInfo {
    pub id: i32,
    pub session_id: String,
    pub original_length: usize,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encrypted_bases: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct SessionSequencesResponse {
    pub sessions: Vec<SessionSequenceGroup>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct StoreSessionSequenceRequest {
    pub session_id: String,
    pub encrypted_bases: Vec<String>,
}

#[derive(Serialize, JsonSchema)]
pub struct StoreSessionSequenceResponse {
    pub sequence: SessionSequenceInfo,
}

#[derive(Serialize, JsonSchema)]
pub struct SessionsResponse {
    pub sessions: Vec<GenomicsSession>,
}

#[derive(Serialize, JsonSchema)]
pub struct FifoStatusResponse {
    pub capacity: usize,
    pub used: usize,
    pub locked: bool,
    pub jobs: Vec<FifoJobStatus>,
}

#[derive(Serialize, JsonSchema)]
pub struct FifoJobStatus {
    pub id: u64,
    pub position: usize,
    pub session_id: String,
    pub job_type: String,
    pub state: String,
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
    pub skipped_results: Vec<SkippedComparison>,
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
    pub skipped_results: Vec<SkippedComparison>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct CompareSessionSequencesRequest {
    pub session_id: String,
    #[serde(default)]
    pub encrypted_sequence: Option<String>,
    #[serde(default)]
    pub encrypted_bases: Option<Vec<String>>,
}

#[derive(Serialize, JsonSchema)]
pub struct CompareSessionSequencesResponse {
    pub encrypted_result_items: Vec<Vec<String>>,
    pub compared_sequences: usize,
    pub sequences: Vec<SessionSequenceInfo>,
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

fn process_hamming(
    req: ProcessRequest,
    pattern: RiskPattern,
    keys: SessionKeys,
) -> Result<ProcessResponse, ApiError> {
    let _timer = FunctionTimer::start("process_hamming");
    let enc_seq = deserialize_encrypted_sequence(
        req.encrypted_sequence.as_deref(),
        req.encrypted_bases.as_deref(),
    )?;
    enforce_sequence_limit(enc_seq.len(), MAX_HAMMING_SEQUENCE_LEN, "Hamming")?;
    let pattern = parse_pattern(&pattern.sequence).map_err(bad_request)?;
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
    enforce_sequence_limit(enc_input.len(), MAX_HAMMING_SEQUENCE_LEN, "Hamming")?;
    let public_key = decode_public_key(&keys.public_key)?;
    let server_key = decode_server_key(&keys.server_key)?;
    let response_patterns = patterns.clone();
    let (comparable_patterns, skipped_results) =
        prepare_risk_patterns_for_all_query(&patterns, enc_input.len(), req.pattern_id.is_none())?;
    let patterns_to_compare = comparable_patterns
        .iter()
        .map(|(_, pattern)| pattern.clone())
        .collect::<Vec<_>>();
    let encrypted_patterns = encrypt_risk_patterns(&patterns_to_compare, &public_key)?;

    let results = with_server_key(server_key, move || {
        compare_against_encrypted_patterns(&enc_input, &encrypted_patterns)
    })?;

    let serialized_results: Vec<Vec<String>> = results
        .par_iter()
        .map(|distances| serialize_fhe_items(distances))
        .collect::<Result<_, _>>()?;
    let mut encrypted_result_items = vec![Vec::new(); response_patterns.len()];
    for ((index, _), items) in comparable_patterns.iter().zip(serialized_results) {
        encrypted_result_items[*index] = items;
    }

    Ok(CompareDatabaseResponse {
        encrypted_result_items,
        compared_sequences: response_patterns.len(),
        patterns: response_patterns,
        skipped_results,
    })
}

fn process_levenshtein(
    req: ProcessRequest,
    pattern: RiskPattern,
    keys: SessionKeys,
) -> Result<ProcessLevenshteinResponse, ApiError> {
    let _timer = FunctionTimer::start("process_levenshtein");
    let enc_seq = deserialize_encrypted_sequence(
        req.encrypted_sequence.as_deref(),
        req.encrypted_bases.as_deref(),
    )?;
    enforce_sequence_limit(enc_seq.len(), MAX_LEVENSHTEIN_SEQUENCE_LEN, "Levenshtein")?;
    let pattern = parse_pattern(&pattern.sequence).map_err(bad_request)?;
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
    enforce_sequence_limit(enc_input.len(), MAX_LEVENSHTEIN_SEQUENCE_LEN, "Levenshtein")?;
    let public_key = decode_public_key(&keys.public_key)?;
    let server_key = decode_server_key(&keys.server_key)?;
    let response_patterns = patterns.clone();
    let (comparable_patterns, skipped_results) =
        prepare_risk_patterns_for_all_query(&patterns, enc_input.len(), req.pattern_id.is_none())?;
    let patterns_to_compare = comparable_patterns
        .iter()
        .map(|(_, pattern)| pattern.clone())
        .collect::<Vec<_>>();
    let encrypted_patterns = encrypt_risk_patterns(&patterns_to_compare, &public_key)?;

    let results = with_server_key(server_key, move || {
        compare_against_encrypted_patterns_levenshtein(&enc_input, &encrypted_patterns)
    })?;

    let serialized_results: Vec<Vec<String>> = results
        .par_iter()
        .map(|distance| serialize_fhe_items(std::slice::from_ref(distance)))
        .collect::<Result<_, _>>()?;
    let mut encrypted_result_items = vec![Vec::new(); response_patterns.len()];
    for ((index, _), items) in comparable_patterns.iter().zip(serialized_results) {
        encrypted_result_items[*index] = items;
    }

    Ok(CompareDatabaseLevenshteinResponse {
        encrypted_result_items,
        compared_sequences: response_patterns.len(),
        patterns: response_patterns,
        skipped_results,
    })
}

fn compare_session_sequences_hamming(
    req: CompareSessionSequencesRequest,
    sequences: Vec<StoredSessionSequence>,
    keys: SessionKeys,
) -> Result<CompareSessionSequencesResponse, ApiError> {
    let _timer = FunctionTimer::start("compare_session_sequences_hamming");
    let enc_input = deserialize_encrypted_sequence(
        req.encrypted_sequence.as_deref(),
        req.encrypted_bases.as_deref(),
    )?;
    enforce_sequence_limit(enc_input.len(), MAX_HAMMING_SEQUENCE_LEN, "Hamming")?;
    let server_key = decode_server_key(&keys.server_key)?;
    let response_sequences: Vec<SessionSequenceInfo> = sequences
        .iter()
        .map(|sequence| sequence.info.clone())
        .collect();
    let encrypted_sequences = sequences
        .par_iter()
        .map(|sequence| {
            let encrypted = deserialize_encrypted_sequence(None, Some(&sequence.encrypted_bases))?;
            enforce_sequence_limit(encrypted.len(), MAX_HAMMING_SEQUENCE_LEN, "Hamming")?;
            Ok(encrypted)
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    let results = with_server_key(server_key, move || {
        encrypted_sequences
            .par_iter()
            .map(|stored_sequence| {
                if enc_input.len() >= stored_sequence.len() {
                    homomorphic_sliding_window_encrypted(&enc_input, stored_sequence)
                } else {
                    homomorphic_sliding_window_encrypted(stored_sequence, &enc_input)
                }
            })
            .collect::<Result<Vec<_>, ApiError>>()
    })?;

    let encrypted_result_items: Vec<Vec<String>> = results
        .par_iter()
        .map(|distances| serialize_fhe_items(distances))
        .collect::<Result<_, _>>()?;

    Ok(CompareSessionSequencesResponse {
        encrypted_result_items,
        compared_sequences: response_sequences.len(),
        sequences: response_sequences,
    })
}

fn compare_session_sequences_levenshtein(
    req: CompareSessionSequencesRequest,
    sequences: Vec<StoredSessionSequence>,
    keys: SessionKeys,
) -> Result<CompareSessionSequencesResponse, ApiError> {
    let _timer = FunctionTimer::start("compare_session_sequences_levenshtein");
    let enc_input = deserialize_encrypted_sequence(
        req.encrypted_sequence.as_deref(),
        req.encrypted_bases.as_deref(),
    )?;
    enforce_sequence_limit(enc_input.len(), MAX_LEVENSHTEIN_SEQUENCE_LEN, "Levenshtein")?;
    let server_key = decode_server_key(&keys.server_key)?;
    let response_sequences: Vec<SessionSequenceInfo> = sequences
        .iter()
        .map(|sequence| sequence.info.clone())
        .collect();
    let encrypted_sequences = sequences
        .par_iter()
        .map(|sequence| {
            let encrypted = deserialize_encrypted_sequence(None, Some(&sequence.encrypted_bases))?;
            enforce_sequence_limit(encrypted.len(), MAX_LEVENSHTEIN_SEQUENCE_LEN, "Levenshtein")?;
            Ok(encrypted)
        })
        .collect::<Result<Vec<_>, ApiError>>()?;

    let results = with_server_key(server_key, move || {
        encrypted_sequences
            .par_iter()
            .map(|stored_sequence| {
                if enc_input.len() >= stored_sequence.len() {
                    homomorphic_levenshtein_distance_encrypted(&enc_input, stored_sequence)
                } else {
                    homomorphic_levenshtein_distance_encrypted(stored_sequence, &enc_input)
                }
            })
            .collect::<Result<Vec<_>, ApiError>>()
    })?;

    let encrypted_result_items: Vec<Vec<String>> = results
        .par_iter()
        .map(|distance| serialize_fhe_items(std::slice::from_ref(distance)))
        .collect::<Result<_, _>>()?;

    Ok(CompareSessionSequencesResponse {
        encrypted_result_items,
        compared_sequences: response_sequences.len(),
        sequences: response_sequences,
    })
}

// API Handler
pub async fn fifo_status_handler() -> Result<axum::Json<FifoStatusResponse>, ApiError> {
    let _timer = FunctionTimer::start("fifo_status_handler");
    Ok(axum::Json(processing_queue().snapshot()?))
}

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

pub async fn create_pattern_handler(
    axum::Json(req): axum::Json<CreateRiskPatternRequest>,
) -> Result<axum::Json<CreateRiskPatternResponse>, ApiError> {
    let _timer = FunctionTimer::start("create_pattern_handler");
    let job = enqueue_processing_job(req.session_id.as_deref(), "Pattern Insert").await?;
    job.wait_turn().await?;
    let pattern = create_risk_pattern(req.sequence).await?;
    drop(job);
    Ok(axum::Json(CreateRiskPatternResponse { pattern }))
}

pub async fn session_sequences_handler() -> Result<axum::Json<SessionSequencesResponse>, ApiError> {
    let _timer = FunctionTimer::start("session_sequences_handler");
    let sessions = list_session_sequence_groups().await?;
    Ok(axum::Json(SessionSequencesResponse { sessions }))
}

pub async fn store_session_sequence_handler(
    axum::Json(req): axum::Json<StoreSessionSequenceRequest>,
) -> Result<axum::Json<StoreSessionSequenceResponse>, ApiError> {
    let _timer = FunctionTimer::start("store_session_sequence_handler");
    let job =
        enqueue_processing_job(Some(req.session_id.as_str()), "Session Sequence Insert").await?;
    job.wait_turn().await?;
    let sequence = store_session_sequence(req.session_id, req.encrypted_bases).await?;
    drop(job);
    Ok(axum::Json(StoreSessionSequenceResponse { sequence }))
}

pub async fn encrypt_handler(
    axum::Json(req): axum::Json<EncryptRequest>,
) -> Result<axum::Json<EncryptResponse>, ApiError> {
    let _timer = FunctionTimer::start("encrypt_handler");
    let job = enqueue_processing_job(req.session_id.as_deref(), "Server Encrypt").await?;
    job.wait_turn().await?;
    let public_key =
        resolve_public_key(req.session_id.as_deref(), req.public_key.as_deref()).await?;
    let response = tokio::task::spawn_blocking(move || {
        let _job = job;
        encrypt_sequence(req, public_key)
    })
    .await
    .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn process_handler(
    axum::Json(req): axum::Json<ProcessRequest>,
) -> Result<axum::Json<ProcessResponse>, ApiError> {
    let _timer = FunctionTimer::start("process_handler");
    let job = enqueue_processing_job(req.session_id.as_deref(), "Hamming").await?;
    job.wait_turn().await?;
    let keys = resolve_session_keys(
        req.session_id.as_deref(),
        req.public_key.as_deref(),
        req.server_key.as_deref(),
    )
    .await?;
    let pattern = load_single_risk_pattern(req.pattern_id).await?;
    let response = tokio::task::spawn_blocking(move || {
        let _job = job;
        process_hamming(req, pattern, keys)
    })
    .await
    .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn compare_database_handler(
    axum::Json(req): axum::Json<CompareDatabaseRequest>,
) -> Result<axum::Json<CompareDatabaseResponse>, ApiError> {
    let _timer = FunctionTimer::start("compare_database_handler");
    let job = enqueue_processing_job(req.session_id.as_deref(), "Hamming DB").await?;
    job.wait_turn().await?;
    let keys = resolve_session_keys(
        req.session_id.as_deref(),
        req.public_key.as_deref(),
        req.server_key.as_deref(),
    )
    .await?;
    let patterns = load_risk_patterns(req.pattern_id).await?;
    if patterns.is_empty() {
        return Err(bad_request("No risk patterns found for selection"));
    }
    let response = tokio::task::spawn_blocking(move || {
        let _job = job;
        compare_database(req, patterns, keys)
    })
    .await
    .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn process_levenshtein_handler(
    axum::Json(req): axum::Json<ProcessRequest>,
) -> Result<axum::Json<ProcessLevenshteinResponse>, ApiError> {
    let _timer = FunctionTimer::start("process_levenshtein_handler");
    let job = enqueue_processing_job(req.session_id.as_deref(), "Levenshtein").await?;
    job.wait_turn().await?;
    let keys = resolve_session_keys(
        req.session_id.as_deref(),
        req.public_key.as_deref(),
        req.server_key.as_deref(),
    )
    .await?;
    let pattern = load_single_risk_pattern(req.pattern_id).await?;
    let response = tokio::task::spawn_blocking(move || {
        let _job = job;
        process_levenshtein(req, pattern, keys)
    })
    .await
    .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn compare_database_levenshtein_handler(
    axum::Json(req): axum::Json<CompareDatabaseRequest>,
) -> Result<axum::Json<CompareDatabaseLevenshteinResponse>, ApiError> {
    let _timer = FunctionTimer::start("compare_database_levenshtein_handler");
    let job = enqueue_processing_job(req.session_id.as_deref(), "Levenshtein DB").await?;
    job.wait_turn().await?;
    let keys = resolve_session_keys(
        req.session_id.as_deref(),
        req.public_key.as_deref(),
        req.server_key.as_deref(),
    )
    .await?;
    let patterns = load_risk_patterns(req.pattern_id).await?;
    if patterns.is_empty() {
        return Err(bad_request("No risk patterns found for selection"));
    }
    let response = tokio::task::spawn_blocking(move || {
        let _job = job;
        compare_database_levenshtein(req, patterns, keys)
    })
    .await
    .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn compare_session_sequences_hamming_handler(
    axum::Json(req): axum::Json<CompareSessionSequencesRequest>,
) -> Result<axum::Json<CompareSessionSequencesResponse>, ApiError> {
    let _timer = FunctionTimer::start("compare_session_sequences_hamming_handler");
    let job = enqueue_processing_job(Some(req.session_id.as_str()), "Session Hamming").await?;
    job.wait_turn().await?;
    let keys = load_session_keys(&req.session_id).await?;
    let sequences = load_session_sequences(&req.session_id).await?;
    let response = tokio::task::spawn_blocking(move || {
        let _job = job;
        compare_session_sequences_hamming(req, sequences, keys)
    })
    .await
    .map_err(join_error)??;
    Ok(axum::Json(response))
}

pub async fn compare_session_sequences_levenshtein_handler(
    axum::Json(req): axum::Json<CompareSessionSequencesRequest>,
) -> Result<axum::Json<CompareSessionSequencesResponse>, ApiError> {
    let _timer = FunctionTimer::start("compare_session_sequences_levenshtein_handler");
    let job = enqueue_processing_job(Some(req.session_id.as_str()), "Session Levenshtein").await?;
    job.wait_turn().await?;
    let keys = load_session_keys(&req.session_id).await?;
    let sequences = load_session_sequences(&req.session_id).await?;
    let response = tokio::task::spawn_blocking(move || {
        let _job = job;
        compare_session_sequences_levenshtein(req, sequences, keys)
    })
    .await
    .map_err(join_error)??;
    Ok(axum::Json(response))
}
