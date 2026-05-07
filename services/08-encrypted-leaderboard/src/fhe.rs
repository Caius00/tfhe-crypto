use rayon::prelude::*;
use rayon::ThreadPool;
use std::sync::Arc;
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheBool, FheUint16, FheUint8, ServerKey};

// FHE-Compute-Kontext einer Session.
//
// Hält den dekomprimierten ServerKey und einen eigenen rayon-Pool, dessen
// Worker-Threads beim Start `tfhe::set_server_key` aufrufen. Dadurch ist der
// Schlüssel auf jedem Thread bereits installiert, wenn FHE-Ops parallel
// (Sort, Rank) ausgeführt werden — kein wiederholtes Dekomprimieren oder
// Set-Per-Call nötig.
pub struct FheEngine {
    pool: ThreadPool,
    _key: Arc<ServerKey>, // hält den ServerKey solange der Pool lebt
}

impl FheEngine {
    // Baut die Engine aus den vom Client gesendeten ServerKey-Bytes auf.
    // Dekomprimierung passiert genau einmal pro Session (teuer).
    pub fn from_compressed_bytes(bytes: &[u8]) -> Result<Self, String> {
        let compressed: CompressedServerKey =
            bincode::deserialize(bytes).map_err(|e| format!("Invalid server key: {e}"))?;
        let key = Arc::new(compressed.decompress());
        Self::with_key(key)
    }

    // Erzeugt den dedizierten rayon-Pool und installiert den ServerKey
    // auf jedem Worker-Thread (start_handler läuft genau einmal pro Thread).
    fn with_key(key: Arc<ServerKey>) -> Result<Self, String> {
        let key_for_init = Arc::clone(&key);
        let pool = rayon::ThreadPoolBuilder::new()
            .start_handler(move |_| {
                tfhe::set_server_key((*key_for_init).clone());
            })
            .build()
            .map_err(|e| format!("Failed to build FHE pool: {e}"))?;
        Ok(Self { pool, _key: key })
    }

    // Führt einen Closure auf dem FHE-Pool aus (ServerKey ist auf allen Workern gesetzt)
    fn install<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        self.pool.install(f)
    }

    // Liefert das bessere von zwei (Score, ID)-Paaren zurück (Maximum nach Score).
    // Genutzt bei Re-Submits desselben Spielers — der höhere Score überschreibt.
    pub fn keep_max(
        &self,
        old_score: &[u8],
        old_id: &[u8],
        new_score: &[u8],
        new_id: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        self.install(|| {
            let os: FheUint16 = bincode::deserialize(old_score)
                .map_err(|e| format!("Deser old_score: {e}"))?;
            let ns: FheUint16 = bincode::deserialize(new_score)
                .map_err(|e| format!("Deser new_score: {e}"))?;
            let oi: FheUint8 = bincode::deserialize(old_id)
                .map_err(|e| format!("Deser old_id: {e}"))?;
            let ni: FheUint8 = bincode::deserialize(new_id)
                .map_err(|e| format!("Deser new_id: {e}"))?;

            let new_wins = os.lt(&ns);
            let kept_s = new_wins.if_then_else(&ns, &os);
            let kept_i = new_wins.if_then_else(&ni, &oi);

            Ok((
                bincode::serialize(&kept_s).map_err(|e| format!("Ser score: {e}"))?,
                bincode::serialize(&kept_i).map_err(|e| format!("Ser id: {e}"))?,
            ))
        })
    }

    // Sortiert (Score, ID)-Paare absteigend nach Score per Batcher's Odd-Even Mergesort.
    // Pro Layer laufen die Compare-and-Swap-Paare echt parallel auf dem Pool —
    // Latenz O(log² n) statt O(n²). Ciphertexts werden nur einmal deserialisiert
    // und am Ende einmal serialisiert (Layer-Schritte arbeiten in-memory).
    pub fn sort_by_score_desc(
        &self,
        pairs: &mut [(Vec<u8>, Vec<u8>)],
    ) -> Result<(), String> {
        let n = pairs.len();
        if n < 2 {
            return Ok(());
        }

        self.install(|| -> Result<(), String> {
            // 1) Eingabe parallel deserialisieren
            let mut scores: Vec<FheUint16> = pairs
                .par_iter()
                .map(|(s, _)| {
                    bincode::deserialize::<FheUint16>(s)
                        .map_err(|e| format!("Deser score: {e}"))
                })
                .collect::<Result<_, _>>()?;
            let mut ids: Vec<FheUint8> = pairs
                .par_iter()
                .map(|(_, i)| {
                    bincode::deserialize::<FheUint8>(i)
                        .map_err(|e| format!("Deser id: {e}"))
                })
                .collect::<Result<_, _>>()?;

            // 2) Sortier-Netzwerk Layer für Layer abarbeiten
            for layer in batcher_layers(n) {
                type Update = (usize, usize, FheUint16, FheUint16, FheUint8, FheUint8);

                // Alle (i,j)-Paare einer Layer sind disjunkt → echte Parallelität möglich.
                // Ergebnisse erst nach der Layer zurückschreiben (kein Aliasing während par_iter).
                let updates: Vec<Update> = layer
                    .par_iter()
                    .map(|&(i, j)| {
                        let s_i = &scores[i];
                        let s_j = &scores[j];
                        let id_i = &ids[i];
                        let id_j = &ids[j];

                        // Absteigend: tausche, wenn s_i kleiner als s_j ist
                        let swap = s_i.lt(s_j);
                        let new_s_i = swap.if_then_else(s_j, s_i);
                        let new_s_j = swap.if_then_else(s_i, s_j);
                        let new_id_i = swap.if_then_else(id_j, id_i);
                        let new_id_j = swap.if_then_else(id_i, id_j);

                        (i, j, new_s_i, new_s_j, new_id_i, new_id_j)
                    })
                    .collect();

                for (i, j, ns_i, ns_j, ni_i, ni_j) in updates {
                    scores[i] = ns_i;
                    scores[j] = ns_j;
                    ids[i] = ni_i;
                    ids[j] = ni_j;
                }
            }

            // 3) Sortiertes Ergebnis zurückserialisieren
            for (idx, pair) in pairs.iter_mut().enumerate() {
                pair.0 = bincode::serialize(&scores[idx])
                    .map_err(|e| format!("Ser score: {e}"))?;
                pair.1 = bincode::serialize(&ids[idx])
                    .map_err(|e| format!("Ser id: {e}"))?;
            }
            Ok(())
        })
    }

    // Rang-Abfrage: liefert pro Position einen verschlüsselten Boolean
    // ("ist die ID hier == target_id?"). E entschlüsselt jede Antwort —
    // jede true-Position ist ein Rang (1-basiert). Unterstützt automatisch
    // Mehrfach-Treffer falls mehrere Einträge dieselbe ID hätten.
    pub fn rank_matches(
        &self,
        sorted: &[(Vec<u8>, Vec<u8>)],
        target_id_bytes: &[u8],
    ) -> Result<Vec<Vec<u8>>, String> {
        if sorted.is_empty() {
            return Ok(Vec::new());
        }
        self.install(|| {
            let target: FheUint8 = bincode::deserialize(target_id_bytes)
                .map_err(|e| format!("Deser target id: {e}"))?;

            sorted
                .par_iter()
                .map(|(_, id_bytes)| {
                    let id: FheUint8 = bincode::deserialize(id_bytes)
                        .map_err(|e| format!("Deser id: {e}"))?;
                    let is_match: FheBool = id.eq(&target);
                    bincode::serialize(&is_match).map_err(|e| format!("Ser bool: {e}"))
                })
                .collect()
        })
    }
}

// Erzeugt die Compare-and-Swap-Layer von Batcher's Odd-Even Mergesort für n Items.
// Jede Layer ist eine Liste disjunkter (i, j)-Paare und kann parallel ausgeführt werden.
// Insgesamt ~ ½·log²(n)·(log²(n)+1) Vergleiche, verteilt auf log²(n) Layer.
fn batcher_layers(n: usize) -> Vec<Vec<(usize, usize)>> {
    let mut layers: Vec<Vec<(usize, usize)>> = Vec::new();

    // nächst-höhere 2er-Potenz
    let mut t = 1;
    while t < n {
        t *= 2;
    }

    let mut p = t / 2;
    while p > 0 {
        let mut q = t / 2;
        let mut r = 0;
        let mut d = p;

        loop {
            let mut layer: Vec<(usize, usize)> = Vec::new();
            let mut i = r;
            while i + d < n {
                if (i & p) == r {
                    layer.push((i, i + d));
                }
                i += 1;
            }
            if !layer.is_empty() {
                layers.push(layer);
            }

            if q == p {
                break;
            }
            d = q - p;
            q /= 2;
            r = p;
        }

        p /= 2;
    }

    layers
}
