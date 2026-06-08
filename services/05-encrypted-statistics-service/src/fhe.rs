use rayon::ThreadPool;
use std::sync::Arc;
use tfhe::ServerKey;

/// FHE-Compute-Kontext für einen einzelnen Request.
///
/// Hält einen dedizierten Rayon-Pool, dessen Worker-Threads beim Start
/// `tfhe::set_server_key` aufrufen. Dadurch sind parallele FHE-Operationen
/// request-sicher: konkurrierende Requests mit unterschiedlichen ServerKeys
/// können sich nicht gegenseitig überschreiben.
pub struct FheEngine {
    pool: ThreadPool,
    key: Arc<ServerKey>,
}

impl FheEngine {
    /// Baut die Engine aus einem bereits dekomprimierten ServerKey auf.
    /// decompress() sollte der Caller bereits in block_in_place ausgeführt haben.
    pub fn from_server_key(key: ServerKey) -> Result<Self, String> {
        Self::with_key(Arc::new(key))
    }

    fn with_key(key: Arc<ServerKey>) -> Result<Self, String> {
        let key_for_init = Arc::clone(&key);
        let pool = rayon::ThreadPoolBuilder::new()
            .start_handler(move |_| {
                tfhe::set_server_key((*key_for_init).clone());
            })
            .build()
            .map_err(|e| format!("Rayon-Pool konnte nicht erstellt werden: {e}"))?;
        Ok(Self { pool, key })
    }

    /// Führt einen Closure auf dem dedizierten FHE-Pool aus.
    ///
    /// Setzt den ServerKey zusätzlich auf dem aufrufenden Thread, da dieser
    /// bei `pool.install` als Worker teilnimmt, aber keinen `start_handler`
    /// erhalten hat.
    pub fn install<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        let key = Arc::clone(&self.key);
        self.pool.install(move || {
            tfhe::set_server_key((*key).clone());
            f()
        })
    }
}
