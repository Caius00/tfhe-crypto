//! Session-State des Statistics-Service.
//!
//! Ermöglicht session-basiertes Key-Caching: der Client lädt den ServerKey
//! einmalig via `POST /session` hoch und bekommt eine UUID zurück. Alle
//! folgenden Berechnungsrequests nutzen nur noch die UUID — der ~80 MB-Overhead
//! pro Request entfällt.
//!
//! Drei Verantwortlichkeiten:
//! 1. `AppState`  — Map `session_id (UUID) → Arc<Session>`, thread-safe.
//! 2. `Session`   — Hält den dekomprimierten Key als `Arc<FheEngine>`.
//! 3. `Janitor`   — Hintergrund-Task, der idle Sessions evicted (OOM-Schutz).
//!
//! Lock-Disziplin: der Map-Lock wird NIE über FHE-Operationen gehalten.
//! Handler ziehen via `get()` einen `Arc<Session>`-Clone, geben den Lock frei
//! und arbeiten danach unlocked.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::fhe::FheEngine;

/// Sessions werden evicted wenn sie länger als 10 Minuten idle sind.
///
/// Jede Session hält einen dekomprimierten ServerKey (mehrere hundert MB).
/// Ohne Eviction würde RAM voll laufen sobald viele Sessions hintereinander
/// angelegt werden.
pub const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Wie oft der Janitor die Map prüft.
pub const JANITOR_INTERVAL: Duration = Duration::from_secs(60);

/// Globaler Service-State. Per Axum `State<AppState>` in jeden Handler injiziert.
///
/// Klonbar — intern wird nur ein `Arc` weitergereicht, alle Klone teilen
/// dieselbe Session-Map.
#[derive(Clone, Default)]
pub struct AppState {
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Legt eine neue Session an und gibt die zugewiesene UUID zurück.
    pub async fn insert(&self, session: Arc<Session>) -> String {
        let session_id = Uuid::new_v4().to_string();
        self.sessions.write().await.insert(session_id.clone(), session);
        session_id
    }

    /// Schlägt eine Session per UUID nach.
    ///
    /// Gibt einen `Arc<Session>`-Clone zurück — Map-Lock wird sofort wieder
    /// freigegeben. Aktualisiert nebenbei `last_access` für den Janitor.
    pub async fn get(&self, session_id: &str) -> Option<Arc<Session>> {
        let session = self.sessions.read().await.get(session_id).cloned()?;
        session.touch();
        Some(session)
    }

    /// Startet den Hintergrund-Janitor, der alle `interval` idle Sessions entfernt.
    pub fn spawn_janitor(&self, idle_timeout: Duration, interval: Duration) {
        let sessions = Arc::clone(&self.sessions);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            // Ersten Tick überspringen — direkt nach Start soll nichts evicted werden.
            tick.tick().await;
            loop {
                tick.tick().await;
                let now = unix_secs();
                let cutoff = now.saturating_sub(idle_timeout.as_secs());
                let mut map = sessions.write().await;
                map.retain(|id, sess| {
                    let last = sess.last_access.load(Ordering::Relaxed);
                    if last < cutoff {
                        tracing::info!(
                            session_id = %id,
                            idle_secs = now.saturating_sub(last),
                            "evicting idle statistics session"
                        );
                        false
                    } else {
                        true
                    }
                });
            }
        });
    }
}

/// FHE-Kontext einer Session.
///
/// Hält den dekomprimierten ServerKey in einer `FheEngine` (dedizierter
/// Rayon-Pool mit thread-lokalem `set_server_key`). Einmalig teuer beim
/// Anlegen — danach für alle Berechnungsrequests dieser Session wiederverwendet.
pub struct Session {
    pub engine: Arc<FheEngine>,
    pub last_access: AtomicU64,
}

impl Session {
    pub fn new(engine: Arc<FheEngine>) -> Self {
        Self {
            engine,
            last_access: AtomicU64::new(unix_secs()),
        }
    }

    /// Aktualisiert `last_access` auf jetzt — verhindert Eviction aktiver Sessions.
    pub fn touch(&self) {
        self.last_access.store(unix_secs(), Ordering::Relaxed);
    }
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
