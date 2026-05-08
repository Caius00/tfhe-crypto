use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, RwLock};

use crate::fhe::FheEngine;

// Maximale Spielerzahl pro Raum (begrenzt durch die FHE-Sortierdauer)
pub const MAX_ENTRIES: usize = 20;

// Globale State: Raumcode -> Session
#[derive(Clone, Default)]
pub struct AppState {
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    // Session unter einem freien Raumcode einfügen — bei Kollision wird neu gewürfelt
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

    // Session zum Raumcode lookupen (Arc-Clone, damit Handler ohne Map-Lock arbeiten können)
    pub async fn get(&self, code: &str) -> Option<Arc<Session>> {
        self.sessions.read().await.get(code).cloned()
    }
}

// Eine Leaderboard-Session.
//
// `entries` ist die Quelle der Wahrheit (Insertion-Order, mit Plaintext-player_key
// für Deduplizierung).  `sorted` ist die zuletzt fertig berechnete Anzeigereihenfolge,
// die der Hintergrund-Sort schreibt.  Beide Locks werden NIE über länger laufende
// FHE-Ops gehalten — Daten werden geclonet und die Locks früh wieder freigegeben.
pub struct Session {
    pub engine: Arc<FheEngine>,
    pub public_key_b64: String,
    pub entries: RwLock<Vec<Entry>>,
    pub sorted: RwLock<Vec<EncEntry>>,
    pub sort_state: Mutex<SortState>,
}

// Koordinator für den Single-Flight-Sort: nur ein Sort-Task pro Session läuft.
// Während eines Sorts werden weitere Submits in `dirty` zusammengefasst und
// genau einmal nachgezogen, wenn der aktuelle Pass fertig ist.
#[derive(Default)]
pub struct SortState {
    pub running: bool,
    pub dirty: bool,
}

// Roher Eintrag inkl. Plaintext-Key für Deduplizierung pro Spieler
pub struct Entry {
    pub player_key: String,
    pub enc: EncEntry,
}

// Verschlüsselte (Score, ID)-Paare wie sie im Leaderboard stehen
#[derive(Clone)]
pub struct EncEntry {
    pub score: Vec<u8>, // FheUint16 (bincode-serialisiert)
    pub id: Vec<u8>,    // FheUint8  (bincode-serialisiert)
}

// 6-stelliger Raumcode aus Wall-Clock + Sub-Sekunden (kollisionsfrei via Retry-Loop)
fn generate_code() -> String {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let n = (t.as_secs() ^ (t.subsec_nanos() as u64)) % 900_000 + 100_000;
    format!("{n}")
}
