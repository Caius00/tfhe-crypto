//! Shared State des Leaderboard-Services.
//!
//! Drei Verantwortlichkeiten:
//! 1. `AppState`  — globale Map `Raumcode -> Arc<Session>` mit Lookup/Insert/Eviction.
//! 2. `Session`   — Daten und FHE-Engine eines konkreten Raums.
//! 3. `Janitor`   — Hintergrund-Task, der idle Sessions wegräumt (verhindert OOM).
//!
//! Lock-Disziplin: Map und Session-Locks (`entries`, `sorted`) werden NIE über
//! länger laufende FHE-Operationen gehalten. Handler ziehen einen Snapshot,
//! geben den Lock frei und arbeiten dann auf der Kopie weiter.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};

use crate::fhe::FheEngine;

/// Maximale Spielerzahl pro Raum.
///
/// Hartes Limit, weil die FHE-Sortierung in O(log²n) Layern à parallele
/// Compare-and-Swaps läuft und jeder Vergleich auf FheUint16 mehrere
/// Sekunden dauert — bei ~20 Spielern noch im erträglichen Bereich.
pub const MAX_ENTRIES: usize = 20;

/// Idle-Timeout für Sessions.
///
/// Wenn an einer Session 10 Minuten lang kein Request mehr passiert, wird sie
/// vom Janitor entfernt. Grund: jede Session hält den dekomprimierten ServerKey
/// (mehrere hundert MB) — ohne Eviction würden hintereinander erstellte Räume
/// den Pod-Speicher voll laufen lassen und einen OOM-Kill auslösen.
pub const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// Wie oft der Janitor die Map prüft. 60 s ist ein guter Kompromiss zwischen
/// „rechtzeitig aufräumen“ und „nicht permanent die globale Map locken“.
pub const JANITOR_INTERVAL: Duration = Duration::from_secs(60);

/// Globaler Service-State. Wird per Axum `State<AppState>` in jeden Handler injiziert.
///
/// Klonbar, weil intern nur ein `Arc` rumgereicht wird — alle Klone teilen sich
/// dieselbe Session-Map.
#[derive(Clone, Default)]
pub struct AppState {
    // RwLock erlaubt parallele Lookups (read), Insert/Evict braucht write.
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fügt eine Session unter einem freien 6-stelligen Raumcode ein.
    ///
    /// Bei (sehr unwahrscheinlicher) Code-Kollision wird einfach neu gewürfelt.
    /// Der Lock wird über die gesamte Suche gehalten — kurze Operation, daher OK.
    pub async fn insert_with_unique_code(&self, session: Arc<Session>) -> String {
        let mut sessions = self.sessions.write().await;
        loop {
            let code = generate_code();
            if !sessions.contains_key(&code) {
                sessions.insert(code.clone(), session);
                return code;
            }
        }
    }

    /// Schlägt eine Session zum Raumcode nach.
    ///
    /// Wichtig: gibt einen `Arc<Session>`-Clone zurück, NICHT eine Referenz —
    /// dadurch geben wir den Map-Lock sofort frei und der Handler kann beliebig
    /// lang auf der Session arbeiten, ohne andere Requests zu blockieren.
    ///
    /// Aktualisiert nebenbei `last_access`, damit der Janitor weiß, dass diese
    /// Session noch aktiv genutzt wird.
    pub async fn get(&self, code: &str) -> Option<Arc<Session>> {
        let session = self.sessions.read().await.get(code).cloned()?;
        session.touch();
        Some(session)
    }

    /// Startet den Hintergrund-Janitor, der alle `interval` Sekunden idle Sessions
    /// entfernt (`last_access` älter als `idle_timeout`).
    ///
    /// Wird vom Binary (`main.rs`) gestartet. Tests starten ihn nicht — dort hält
    /// der Prozess sowieso nur kurz und Sessions müssen nicht weg-evicted werden.
    pub fn spawn_janitor(&self, idle_timeout: Duration, interval: Duration) {
        let sessions = Arc::clone(&self.sessions);
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            // `tokio::time::interval` feuert den ersten Tick sofort — den
            // überspringen wir, damit der Janitor frühestens nach `interval`
            // das erste Mal aktiv wird (sonst räumen wir möglicherweise eine
            // gerade erst angelegte Session direkt wieder aus).
            tick.tick().await;
            loop {
                tick.tick().await;
                let now = unix_secs();
                let cutoff = now.saturating_sub(idle_timeout.as_secs());

                // Write-Lock auf die Map ist OK, weil `retain` nur kurz läuft —
                // wir machen hier keine FHE-Ops, nur Vergleiche und Drops.
                let mut map = sessions.write().await;
                map.retain(|code, sess| {
                    let last = sess.last_access.load(Ordering::Relaxed);
                    if last < cutoff {
                        // Beim Drop des Arc wird der ServerKey + alle Ciphertexts
                        // freigegeben — exakt das Speicher-Leak, das wir verhindern wollen.
                        tracing::info!(
                            code = %code,
                            idle_secs = now.saturating_sub(last),
                            "evicting idle leaderboard session"
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

/// Daten und FHE-Kontext eines einzelnen Leaderboard-Raums.
///
/// Felder:
/// - `engine`: Dekomprimierter ServerKey + dedizierter rayon-Pool (teuer beim Anlegen).
/// - `public_key_b64`: Wird unverändert an Spieler weitergereicht, damit die ihre Scores
///   mit dem richtigen Schlüssel verschlüsseln.
/// - `entries`: Quelle der Wahrheit — Insertion-Order, inkl. Plaintext-`player_key`
///   für serverseitiges Dedup (mehrfaches Einreichen desselben Spielers).
/// - `sorted`: Zuletzt fertig berechnete Anzeigereihenfolge (vom Hintergrund-Sort).
///   Wird nur gelesen wenn nicht leer, sonst fallback auf `entries`.
/// - `sort_state`: Koordiniert den Single-Flight-Sort (siehe `SortState`).
/// - `last_access`: Unix-Sekunden des letzten Lookups — vom Janitor ausgewertet.
pub struct Session {
    pub engine: Arc<FheEngine>,
    pub public_key_b64: String,
    pub entries: RwLock<Vec<Entry>>,
    pub sorted: RwLock<Vec<EncEntry>>,
    pub sort_state: Mutex<SortState>,
    pub last_access: AtomicU64,
}

impl Session {
    /// Baut eine neue Session und setzt `last_access` auf jetzt, damit sie nicht
    /// direkt nach dem Anlegen schon als idle gilt.
    pub fn new(engine: Arc<FheEngine>, public_key_b64: String) -> Self {
        Self {
            engine,
            public_key_b64,
            entries: RwLock::default(),
            sorted: RwLock::default(),
            sort_state: Mutex::default(),
            last_access: AtomicU64::new(unix_secs()),
        }
    }

    /// Aktualisiert `last_access` auf jetzt — wird bei jedem erfolgreichen Lookup
    /// in `AppState::get` aufgerufen. `Relaxed` reicht: wir brauchen keinerlei
    /// Ordering-Garantien zwischen Threads, nur einen ungefähren „zuletzt benutzt“-Stempel.
    pub fn touch(&self) {
        self.last_access.store(unix_secs(), Ordering::Relaxed);
    }
}

/// Koordinator für den Single-Flight-Sort: nur ein Sort-Task pro Session läuft
/// gleichzeitig. Wenn während eines laufenden Sorts ein neuer Submit kommt,
/// wird `dirty` gesetzt und der laufende Task zieht GENAU EINMAL einen weiteren
/// Pass nach. Effekt: Burst-Submits kosten höchstens „current+1“ Sorts, statt sich
/// in einer Queue zu stapeln.
#[derive(Default)]
pub struct SortState {
    pub running: bool,
    pub dirty: bool,
}

/// Roher Eintrag inkl. Plaintext-`player_key`.
///
/// Der `player_key` ist KLARTEXT und dient nur zur server-seitigen Dedup —
/// damit derselbe Spieler bei mehrfachem Submit nicht doppelt in der Liste
/// erscheint, sondern sein Score per FHE-Max aktualisiert wird.
/// Da E (der Raum-Ersteller) sowieso weiß welche Spieler im Raum sind,
/// leakt das keine relevante Information.
pub struct Entry {
    pub player_key: String,
    pub enc: EncEntry,
}

/// Verschlüsseltes (Score, ID)-Paar, wie es im Leaderboard steht.
///
/// Beide Felder sind bincode-serialisierte FHE-Ciphertexts (binärer Blob),
/// werden für den HTTP-Transport base64-codiert.
#[derive(Clone)]
pub struct EncEntry {
    pub score: Vec<u8>, // FheUint16 (bincode-serialisiert) — Score 0..=65535
    pub id: Vec<u8>,    // FheUint8  (bincode-serialisiert) — ID    0..=255
}

/// 6-stelliger Raumcode aus aktueller Wall-Clock-Zeit + Sub-Sekunden gemischt.
///
/// Kein kryptographisch sicherer Zufallswert — bei Kollision retried der
/// Aufrufer (`insert_with_unique_code`), das reicht für ~10⁵ gleichzeitige Räume.
fn generate_code() -> String {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    // XOR mischt die Sub-Sekunden rein, damit zwei /create-Calls in derselben
    // Sekunde nicht garantiert denselben Code liefern.
    let n = (t.as_secs() ^ (t.subsec_nanos() as u64)) % 900_000 + 100_000;
    format!("{n}")
}

/// Hilfsfunktion: aktuelle Unix-Zeit in Sekunden. Bei (theoretisch unmöglichem)
/// Zeitsprung vor 1970 geben wir 0 zurück, damit nichts panickt.
fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
