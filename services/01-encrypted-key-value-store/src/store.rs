//! Persistenz- und Sitzungs-Layer für den Encrypted Key-Value Store.
//!
//! Zwei Verantwortlichkeiten:
//!
//! 1. **Redis-IO** für die eigentlichen Einträge. Schlüssel-Schema:
//!    `kv:{session_id}:{entry_uuid}` → bincode-`StoredEntry` (verschlüsselter
//!    Schlüssel + verschlüsselter Wert). TTL pro Eintrag (Klartext-Metadatum).
//!
//! 2. **Server-Key-Map** im Prozessspeicher: jede Session hat einen
//!    dekomprimierten `tfhe::ServerKey`, der für homomorphe Vergleiche nötig
//!    ist. Bei Pod-Restart sind diese Keys weg — der Client muss dann eine
//!    neue Session öffnen. Vertretbarer Kompromiss, weil der Cluster via KEDA
//!    auf maximal eine Replica skaliert.
//!
//! Die teuren FHE-Operationen passieren bewusst nicht hier, sondern in
//! `routes.rs` — dieser Modulteil ist reine IO, damit der Lese-Pfad in
//! `block_in_place` sauber von der asynchronen Redis-Phase trennbar bleibt.

use crate::models::AppError;
use dotenvy::from_path;
use redis::{
    AsyncCommands, AsyncIter, Client, ConnectionAddr, ConnectionInfo, RedisConnectionInfo,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use tfhe::ServerKey;
use tokio::sync::RwLock;
use uuid::Uuid;

/// In Redis abgelegtes Tupel: verschlüsselter Schlüssel + verschlüsselter Wert.
/// Wir speichern den Schlüssel mit, weil der Get-/Exists-Pfad ihn homomorph
/// gegen den angefragten Schlüssel vergleichen muss.
#[derive(Serialize, Deserialize)]
pub struct StoredEntry {
    pub key_chunks: Vec<Vec<u8>>,
    pub value_chunks: Vec<Vec<u8>>,
}

/// Prefix-Helfer — eine Stelle für das Redis-Schema, statt es in mehreren
/// Funktionen string-zu-bauen.
fn session_prefix(session_id: &str) -> String {
    format!("kv:{session_id}:")
}

fn session_match_pattern(session_id: &str) -> String {
    format!("kv:{session_id}:*")
}

/// Redis-Key, unter dem die bincode-`CompressedServerKey`-Bytes einer Session
/// hinterlegt werden. Eigener Namespace (`kvs:`) damit er nicht mit den
/// Entry-Keys (`kv:`) kollidiert — auch nicht beim SCAN-Pattern oben.
fn session_server_key_key(session_id: &str) -> String {
    format!("kvs:{session_id}")
}

/// Baut den Redis-Client aus den ENV-Variablen, die das Helm-Chart setzt.
/// Bevorzugt `REDIS_URL`, fällt sonst auf `REDIS_HOST`/`PORT`/`PASSWORD` zurück.
///
/// Gibt neben dem Client eine kurze Endpoint-Beschreibung (`host:port`) zurück,
/// damit Logs eindeutig zeigen, wohin der Service tatsächlich verbindet — das
/// hat in der Vergangenheit Stunden Debugging gespart, wenn der Pod auf das
/// falsche Redis (oder gar `127.0.0.1`) zeigte.
fn build_redis_client() -> Result<(Client, String), redis::RedisError> {
    if let Ok(url) = env::var("REDIS_URL") {
        // URL parsen wir nicht selbst — wir loggen sie ohne Passwort, indem wir
        // alles nach dem ersten `@` als sichtbaren Endpoint nehmen. Wenn kein
        // `@` drin ist, ist die URL ohnehin passwortfrei.
        let visible = url
            .split_once('@')
            .map(|(_, host)| host.to_string())
            .unwrap_or_else(|| url.clone());
        return Ok((Client::open(url)?, format!("REDIS_URL→{visible}")));
    }

    let host = env::var("REDIS_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("REDIS_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(6379u16);
    let password = env::var("REDIS_PASSWORD").ok().filter(|p| !p.is_empty());

    let endpoint = format!("{host}:{port}");
    let client = Client::open(ConnectionInfo {
        addr: ConnectionAddr::Tcp(host, port),
        redis: RedisConnectionInfo {
            db: 0,
            username: None,
            password,
        },
    })?;
    Ok((client, endpoint))
}

pub struct AppState {
    /// Pool-fähiger Redis-Client (eine Connection wird per
    /// `get_multiplexed_async_connection` pro Operation geholt).
    pub client: Client,
    /// Lesbare Beschreibung des Redis-Endpoints — nur fürs Logging.
    pub redis_endpoint: String,
    /// Default-TTL für Einträge, wenn der Client keine eigene angibt.
    pub default_ttl_sec: u64,
    /// TTL für die in Redis hinterlegten ServerKeys — länger als die
    /// Entry-TTL, damit eine Session, die gerade Pause macht, nicht aus
    /// Versehen vor ihren Einträgen verfällt.
    pub session_ttl_sec: u64,
    /// Hot-Cache: pro Session der dekomprimierte ServerKey für homomorphe
    /// Operationen. Wird auf Cache-Miss aus Redis nachgeladen, damit ein
    /// Pod-Restart bestehende Sessions nicht killt.
    pub server_keys: RwLock<HashMap<String, ServerKey>>,
}

pub type SharedState = Arc<AppState>;

impl AppState {
    /// Initialisiert den Service-State: liest ENV, baut den Redis-Client und
    /// legt eine leere Session-Map an.
    ///
    /// Panic-Verhalten: bei fehlgeschlagener Redis-Konfiguration sterben wir
    /// schon beim Boot — das ist gewollt, denn ohne Redis kann der Service
    /// keine Anfrage sinnvoll bedienen. Der k8s-Liveness-Check würde dasselbe
    /// Ergebnis liefern, nur später.
    pub fn new() -> Self {
        // Lokales Entwickeln: optional eine .env in einem von zwei Pfaden
        // einlesen. In Production (Container) ist beides nicht vorhanden, dann
        // greifen die echten ENV-Variablen aus dem Helm-Chart.
        let _ = from_path("./services/01-encrypted-key-value-store/.env");
        let _ = from_path(".env");

        let default_ttl_sec = env::var("TTL_MINUTES")
            .ok()
            .and_then(|m| m.parse::<u64>().ok())
            .unwrap_or(5)
            * 60;

        // Sessions sollen Pod-Restarts und kurze Pausen überleben — Default
        // eine Stunde, per ENV verstellbar.
        let session_ttl_sec = env::var("SESSION_TTL_MINUTES")
            .ok()
            .and_then(|m| m.parse::<u64>().ok())
            .unwrap_or(60)
            * 60;

        let (client, redis_endpoint) = build_redis_client().expect("failed to open Redis client");

        Self {
            client,
            redis_endpoint,
            default_ttl_sec,
            session_ttl_sec,
            server_keys: RwLock::new(HashMap::new()),
        }
    }

    /// Probiert die Redis-Verbindung mit einem `PING`. Wird in `main` direkt
    /// nach dem Bauen des States aufgerufen, damit der Service beim Start
    /// klar sagt, ob er sprechen kann — und nicht erst beim ersten Put/Get.
    pub async fn ping_redis(&self) -> Result<(), redis::RedisError> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let pong: String = redis::cmd("PING").query_async(&mut con).await?;
        if pong != "PONG" {
            return Err(redis::RedisError::from((
                redis::ErrorKind::ResponseError,
                "unexpected PING response",
            )));
        }
        Ok(())
    }

    /// Speichert einen frisch hochgeladenen ServerKey unter einer neuen UUID
    /// und gibt die Session-ID heraus.
    ///
    /// Wir bekommen die Original-`CompressedServerKey`-Bytes mit, weil die
    /// in Redis abgelegt werden, damit Pod-Restarts die Session nicht killen.
    /// Der dekomprimierte ServerKey selbst landet im In-Memory-Hot-Cache
    /// (Dekomprimieren ist teuer und soll nicht pro Request wiederholt werden).
    pub async fn register_session(
        &self,
        compressed_server_key_bytes: Vec<u8>,
        server_key: ServerKey,
    ) -> Result<String, AppError> {
        let session_id = Uuid::new_v4().to_string();

        let mut con = self.client.get_multiplexed_async_connection().await?;
        con.set_ex::<String, Vec<u8>, ()>(
            session_server_key_key(&session_id),
            compressed_server_key_bytes,
            self.session_ttl_sec,
        )
        .await?;

        self.server_keys
            .write()
            .await
            .insert(session_id.clone(), server_key);
        Ok(session_id)
    }

    /// Holt den ServerKey einer Session als Klon raus.
    ///
    /// Hot-Path: aus dem In-Memory-Cache. Cold-Path: aus Redis nachladen,
    /// dekomprimieren, in den Cache stellen. Erst wenn beides fehlt, ist die
    /// Session wirklich abgelaufen → `Unauthorized`.
    ///
    /// Der Aufrufer aktiviert den Key auf einem CPU-Worker-Thread
    /// (`block_in_place`) per `tfhe::set_server_key`.
    pub async fn fetch_server_key(&self, session_id: &str) -> Result<ServerKey, AppError> {
        if let Some(cached) = self.server_keys.read().await.get(session_id).cloned() {
            return Ok(cached);
        }

        // Cold-Path: Redis-Lookup. Decompress ist CPU-bound, deshalb in
        // `block_in_place`, damit der Executor frei bleibt.
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let bytes: Option<Vec<u8>> = con.get(session_server_key_key(session_id)).await?;
        let bytes = bytes.ok_or(AppError::Unauthorized)?;

        let server_key = tokio::task::block_in_place(|| -> Result<ServerKey, AppError> {
            let compressed: tfhe::CompressedServerKey = bincode::deserialize(&bytes)
                .map_err(|e| AppError::InternalError(format!("corrupt session in Redis: {e}")))?;
            Ok(compressed.decompress())
        })?;

        self.server_keys
            .write()
            .await
            .insert(session_id.to_string(), server_key.clone());
        tracing::info!(%session_id, "session rehydrated from redis after cache miss");

        Ok(server_key)
    }

    /// Legt einen Eintrag in Redis ab. Jeder Put bekommt eine eigene UUID,
    /// es gibt keine homomorphe Upsert-Semantik — das wäre ein deutlich
    /// größerer FHE-Aufwand und ist in der Spec als bewusste Limitation
    /// dokumentiert.
    pub async fn put_entry(
        &self,
        session_id: &str,
        key_chunks: Vec<Vec<u8>>,
        value_chunks: Vec<Vec<u8>>,
        ttl_sec: u64,
    ) -> Result<(), AppError> {
        let mut con = self.client.get_multiplexed_async_connection().await?;

        let db_key = format!("{}{}", session_prefix(session_id), Uuid::new_v4());
        let entry = StoredEntry {
            key_chunks,
            value_chunks,
        };
        let bytes = bincode::serialize(&entry)
            .map_err(|e| AppError::InternalError(format!("bincode StoredEntry: {e}")))?;

        con.set_ex::<String, Vec<u8>, ()>(db_key, bytes, ttl_sec)
            .await?;
        Ok(())
    }

    /// Lädt alle nicht-abgelaufenen Einträge dieser Session aus Redis.
    /// Der Aufrufer (routes.rs) iteriert darüber für homomorphe Vergleiche.
    pub async fn load_session_entries(
        &self,
        session_id: &str,
    ) -> Result<Vec<StoredEntry>, AppError> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let pattern = session_match_pattern(session_id);

        // Schlüssel der Session per SCAN-Cursor einsammeln. SCAN ist
        // inkrementell und blockiert Redis nicht — anders als KEYS.
        let mut iter: AsyncIter<String> = con.scan_match(&pattern).await?;
        let mut keys = Vec::new();
        while let Some(k) = iter.next_item().await {
            keys.push(k);
        }
        drop(iter);

        if keys.is_empty() {
            return Ok(Vec::new());
        }

        // MGET für alle Einträge auf einmal — eine Round-Trip statt N.
        let values: Vec<Option<Vec<u8>>> = con.mget(&keys).await?;

        let mut entries = Vec::with_capacity(values.len());
        for v in values.into_iter().flatten() {
            let entry: StoredEntry = bincode::deserialize(&v).map_err(|e| {
                AppError::InternalError(format!("corrupted StoredEntry in Redis: {e}"))
            })?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Löscht alle Einträge dieser Session. Andere Sessions bleiben unberührt
    /// — wichtig für die Mandantentrennung.
    pub async fn clear_session(&self, session_id: &str) -> Result<u64, AppError> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let pattern = session_match_pattern(session_id);

        let mut iter: AsyncIter<String> = con.scan_match(&pattern).await?;
        let mut keys = Vec::new();
        while let Some(k) = iter.next_item().await {
            keys.push(k);
        }
        drop(iter);

        if keys.is_empty() {
            return Ok(0);
        }

        let deleted: u64 = con.del(&keys).await?;
        Ok(deleted)
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
