//! FHE-Compute-Layer einer Session.
//!
//! Diese Datei kapselt alles, was direkt mit dem tfhe-rs ServerKey arbeitet:
//! - `FheEngine`             — hält ServerKey + dedizierten rayon-Pool.
//! - `keep_max`              — FHE-Maximum zweier (Score, ID)-Paare (für Re-Submit).
//! - `sort_by_score_desc`    — FHE-Sortier-Netzwerk (Batcher's Odd-Even Mergesort).
//! - `rank_matches`          — Pro-Position-Gleichheit für die Rang-Abfrage.
//! - `batcher_layers`        — Erzeugt die Compare-and-Swap-Layer des Sort-Netzwerks.
//!
//! Warum ein eigener rayon-Pool pro Session?
//! - tfhe-rs braucht `set_server_key` pro Thread, der FHE-Ops ausführt.
//! - Mit einem dedizierten Pool und `start_handler` setzen wir den Key
//!   genau einmal pro Worker-Thread (statt bei jedem Call neu).
//! - So bleibt der ServerKey thread-lokal verfügbar für `par_iter` etc.
//!
//! Warum bincode + Vec<u8>?
//! - FHE-Ciphertexts werden außerhalb dieser Datei (Handlers/State) als
//!   `Vec<u8>` herumgereicht, damit Locks und Cloning billig bleiben.
//! - Deserialisierung in echte `FheUint*` passiert nur kurz innerhalb von
//!   `install(...)`, also unter installiertem ServerKey.

use rayon::prelude::*;
use rayon::ThreadPool;
use std::sync::Arc;
use tfhe::prelude::*;
use tfhe::{CompressedServerKey, FheBool, FheUint16, FheUint8, ServerKey};

/// FHE-Compute-Kontext einer Session.
///
/// Hält den dekomprimierten ServerKey und einen eigenen rayon-Pool, dessen
/// Worker-Threads beim Start `tfhe::set_server_key` aufrufen. Dadurch ist der
/// Schlüssel auf jedem Thread schon installiert, wenn FHE-Ops parallel
/// (Sort, Rank) ausgeführt werden — kein wiederholtes Dekomprimieren oder
/// Set-Per-Call nötig.
pub struct FheEngine {
    pool: ThreadPool,
    // Underscore-Prefix: Feld wird nicht direkt gelesen, sondern hält nur
    // den `Arc<ServerKey>` am Leben, solange der Pool existiert.
    _key: Arc<ServerKey>,
}

impl FheEngine {
    /// Baut die Engine aus den vom Client gesendeten ServerKey-Bytes auf.
    /// Dekomprimierung passiert genau EINMAL pro Session (teuer, mehrere Sek
    /// und mehrere hundert MB).
    #[tracing::instrument(skip(bytes), fields(bytes_len = bytes.len()))]
    pub fn from_compressed_bytes(bytes: &[u8]) -> Result<Self, String> {
        let compressed: CompressedServerKey =
            bincode::deserialize(bytes).map_err(|e| format!("Invalid server key: {e}"))?;
        let key = Arc::new(compressed.decompress());
        Self::with_key(key)
    }

    /// Erzeugt den dedizierten rayon-Pool und installiert den ServerKey
    /// auf jedem Worker-Thread (start_handler läuft genau einmal pro Thread).
    fn with_key(key: Arc<ServerKey>) -> Result<Self, String> {
        let key_for_init = Arc::clone(&key);
        let pool = rayon::ThreadPoolBuilder::new()
            .start_handler(move |_| {
                // Pro Thread: ServerKey thread-lokal setzen.
                // tfhe-rs verlangt das, sonst panicken FHE-Ops mit "no server key".
                tfhe::set_server_key((*key_for_init).clone());
            })
            .build()
            .map_err(|e| format!("Failed to build FHE pool: {e}"))?;
        Ok(Self { pool, _key: key })
    }

    /// Führt einen Closure auf dem FHE-Pool aus (ServerKey ist auf allen Workern gesetzt).
    /// `pool.install` blockt den aufrufenden Thread bis der Closure fertig ist —
    /// daher darf diese Funktion nur aus `spawn_blocking`-Tasks aufgerufen werden.
    fn install<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R + Send,
        R: Send,
    {
        self.pool.install(f)
    }

    /// Liefert das bessere von zwei (Score, ID)-Paaren zurück (Maximum nach Score).
    ///
    /// Genutzt bei Re-Submits desselben Spielers — der höhere Score überschreibt
    /// den niedrigeren, OHNE dass der Server jemals erfährt welcher höher ist.
    ///
    /// Trick: wir bauen einen verschlüsselten Bool `new_wins = old < new` und
    /// nutzen ihn als Switch für ein `if_then_else` auf Score UND ID — so wandern
    /// beide Felder konsistent durch.
    #[tracing::instrument(skip_all)]
    pub fn keep_max(
        &self,
        old_score: &[u8],
        old_id: &[u8],
        new_score: &[u8],
        new_id: &[u8],
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        self.install(|| {
            // Bytes → FheUint* (deserialisieren unter installiertem ServerKey).
            let os: FheUint16 =
                bincode::deserialize(old_score).map_err(|e| format!("Deser old_score: {e}"))?;
            let ns: FheUint16 =
                bincode::deserialize(new_score).map_err(|e| format!("Deser new_score: {e}"))?;
            let oi: FheUint8 =
                bincode::deserialize(old_id).map_err(|e| format!("Deser old_id: {e}"))?;
            let ni: FheUint8 =
                bincode::deserialize(new_id).map_err(|e| format!("Deser new_id: {e}"))?;

            // Verschlüsselter Vergleich: `new_wins` ist ein FheBool (1 = neuer Score gewinnt).
            let new_wins = os.lt(&ns);
            // Beide Felder synchron durchschalten — Server weiß nicht welcher Branch gewinnt.
            let kept_s = new_wins.if_then_else(&ns, &os);
            let kept_i = new_wins.if_then_else(&ni, &oi);

            // Ergebnis zurück in Bytes packen.
            Ok((
                bincode::serialize(&kept_s).map_err(|e| format!("Ser score: {e}"))?,
                bincode::serialize(&kept_i).map_err(|e| format!("Ser id: {e}"))?,
            ))
        })
    }

    /// Sortiert (Score, ID)-Paare absteigend nach Score per Batcher's Odd-Even Mergesort.
    ///
    /// Warum gerade dieser Algorithmus?
    /// - Datenunabhängig: Vergleichs-Reihenfolge hängt nicht von den (verschlüsselten)
    ///   Werten ab — perfekt für FHE, weil Branches auf Ciphertexts ohnehin nicht gehen.
    /// - Pro Layer sind alle (i,j)-Paare disjunkt → echte Parallelität möglich
    ///   auf dem rayon-Pool. Latenz O(log²n) statt O(n) bei sequenziellem Bubblesort.
    /// - Pro Vergleich: ein FHE-Lt + 4× if_then_else (Score×2, ID×2).
    ///
    /// Ablauf:
    /// 1. Eingabe einmalig deserialisieren (parallel).
    /// 2. Layer für Layer abarbeiten — innerhalb einer Layer parallel.
    ///    Ergebnisse erst NACH der Layer zurückschreiben, damit kein Aliasing
    ///    entsteht (rayon erlaubt sonst keine `&mut`-Zugriffe in `par_iter`).
    /// 3. Sortiertes Ergebnis einmalig serialisieren.
    #[tracing::instrument(skip(self, pairs), fields(n = pairs.len()))]
    pub fn sort_by_score_desc(&self, pairs: &mut [(Vec<u8>, Vec<u8>)]) -> Result<(), String> {
        let n = pairs.len();
        if n < 2 {
            return Ok(());
        }

        self.install(|| -> Result<(), String> {
            // 1) Eingabe parallel deserialisieren — Score und ID jeweils separat,
            //    weil rayon nur eine Quelle iterieren kann.
            let mut scores: Vec<FheUint16> = pairs
                .par_iter()
                .map(|(s, _)| {
                    bincode::deserialize::<FheUint16>(s).map_err(|e| format!("Deser score: {e}"))
                })
                .collect::<Result<_, _>>()?;
            let mut ids: Vec<FheUint8> = pairs
                .par_iter()
                .map(|(_, i)| {
                    bincode::deserialize::<FheUint8>(i).map_err(|e| format!("Deser id: {e}"))
                })
                .collect::<Result<_, _>>()?;

            // 2) Sortier-Netzwerk Layer für Layer abarbeiten.
            for layer in batcher_layers(n) {
                // Tupel-Typ-Alias für besseren Lesefluss in der Map weiter unten.
                type Update = (usize, usize, FheUint16, FheUint16, FheUint8, FheUint8);

                // Alle (i,j)-Paare einer Layer sind disjunkt → echte Parallelität möglich.
                // Wir sammeln Ergebnisse in `updates` und schreiben sie nach der Layer
                // sequenziell zurück — verhindert mutable Aliasing während `par_iter`.
                let updates: Vec<Update> = layer
                    .par_iter()
                    .map(|&(i, j)| {
                        let s_i = &scores[i];
                        let s_j = &scores[j];
                        let id_i = &ids[i];
                        let id_j = &ids[j];

                        // Absteigend sortieren heißt: tausche wenn s_i < s_j.
                        let swap = s_i.lt(s_j);
                        let new_s_i = swap.if_then_else(s_j, s_i);
                        let new_s_j = swap.if_then_else(s_i, s_j);
                        let new_id_i = swap.if_then_else(id_j, id_i);
                        let new_id_j = swap.if_then_else(id_i, id_j);

                        (i, j, new_s_i, new_s_j, new_id_i, new_id_j)
                    })
                    .collect();

                // Sequenzielles Zurückschreiben — keine FHE-Ops mehr, nur Move.
                for (i, j, ns_i, ns_j, ni_i, ni_j) in updates {
                    scores[i] = ns_i;
                    scores[j] = ns_j;
                    ids[i] = ni_i;
                    ids[j] = ni_j;
                }
            }

            // 3) Sortiertes Ergebnis zurückserialisieren (in-place auf `pairs`).
            for (idx, pair) in pairs.iter_mut().enumerate() {
                pair.0 = bincode::serialize(&scores[idx]).map_err(|e| format!("Ser score: {e}"))?;
                pair.1 = bincode::serialize(&ids[idx]).map_err(|e| format!("Ser id: {e}"))?;
            }
            Ok(())
        })
    }

    /// Rang-Abfrage: liefert pro Position einen verschlüsselten Bool
    /// (`true` ⇔ ID an dieser Position passt zum gesuchten `target_id`).
    ///
    /// E entschlüsselt jedes Element der Antwort einzeln — jede `true`-Position
    /// entspricht einem Rang (1-basiert). Unterstützt automatisch Mehrfach-Treffer,
    /// falls dieselbe ID in mehreren Einträgen vorkommt.
    ///
    /// Vergleiche laufen parallel auf dem Pool — pro Eintrag: ein FHE-Eq.
    #[tracing::instrument(skip_all, fields(n = sorted.len()))]
    pub fn rank_matches(
        &self,
        sorted: &[(Vec<u8>, Vec<u8>)],
        target_id_bytes: &[u8],
    ) -> Result<Vec<Vec<u8>>, String> {
        if sorted.is_empty() {
            return Ok(Vec::new());
        }
        self.install(|| {
            // Ziel-ID einmalig deserialisieren — wird bei jedem Vergleich wiederverwendet.
            let target: FheUint8 = bincode::deserialize(target_id_bytes)
                .map_err(|e| format!("Deser target id: {e}"))?;

            // Pro Position: deserialisieren, Eq berechnen, serialisieren.
            // Score-Bytes ignorieren wir hier — die brauchen wir nicht für Gleichheit.
            sorted
                .par_iter()
                .map(|(_, id_bytes)| {
                    let id: FheUint8 =
                        bincode::deserialize(id_bytes).map_err(|e| format!("Deser id: {e}"))?;
                    let is_match: FheBool = id.eq(&target);
                    bincode::serialize(&is_match).map_err(|e| format!("Ser bool: {e}"))
                })
                .collect()
        })
    }
}

/// Erzeugt die Compare-and-Swap-Layer von Batcher's Odd-Even Mergesort für n Items.
///
/// Jede Layer ist eine Liste DISJUNKTER (i, j)-Paare und kann parallel ausgeführt werden.
/// Insgesamt ~ ½·log²(n)·(log²(n)+1) Vergleiche, verteilt auf O(log²n) Layer.
///
/// Implementierung folgt direkt dem Pseudocode von Batcher (1968): zwei
/// ineinander geschachtelte Schleifen über `p` (Halbierung) und `q` (Verfeinerung).
/// Paare außerhalb von `n` (wenn n keine 2er-Potenz ist) werden einfach übersprungen.
fn batcher_layers(n: usize) -> Vec<Vec<(usize, usize)>> {
    let mut layers: Vec<Vec<(usize, usize)>> = Vec::new();

    // Nächst-höhere 2er-Potenz zu n bestimmen — Batcher arbeitet auf einer
    // virtuellen Liste der Länge `t`, Paare mit Index ≥ n werden später gefiltert.
    let mut t = 1;
    while t < n {
        t *= 2;
    }

    // Äußere Schleife: `p` halbiert sich von t/2 bis 1.
    let mut p = t / 2;
    while p > 0 {
        // Innere Schleife: `d` wird zunächst auf `p` gesetzt, dann iterativ verfeinert.
        let mut q = t / 2;
        let mut r = 0;
        let mut d = p;

        loop {
            let mut layer: Vec<(usize, usize)> = Vec::new();
            let mut i = r;
            // Sammle alle Paare (i, i+d), die in dieser Layer verglichen werden müssen.
            // `i & p == r` ist Batchers Auswahlkriterium für die richtige Hälfte.
            while i + d < n {
                if (i & p) == r {
                    layer.push((i, i + d));
                }
                i += 1;
            }
            if !layer.is_empty() {
                layers.push(layer);
            }

            // Abbruch: wenn `q` auf `p` zurückgefallen ist, sind wir mit dieser Verfeinerung durch.
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

#[cfg(test)]
mod tests {
    use super::batcher_layers;
    use std::collections::HashSet;

    /// 0/1-Prinzip: ein Comparator-Netzwerk sortiert genau dann beliebige Eingaben
    /// korrekt, wenn es alle binären Eingaben sortiert. Wir prüfen also alle
    /// 2^n Bit-Sequenzen für n = 0..=8 — ist das Netzwerk dort korrekt, ist es
    /// allgemein korrekt (klassisches Resultat aus Knuth TAOCP Vol. 3, §5.3.4).
    ///
    /// Zusätzlich verifizieren wir die Layer-Invariante: innerhalb einer Layer
    /// darf kein Index doppelt vorkommen (sonst wäre die Parallelisierung unsicher).
    #[test]
    fn batcher_layers_form_a_valid_sorting_network() {
        for n in 0usize..=8 {
            let layers = batcher_layers(n);

            // Invariante: Paare innerhalb einer Layer sind disjunkt + in Range.
            for layer in &layers {
                let mut indices = HashSet::new();
                for &(i, j) in layer {
                    assert!(i < j && j < n, "invalid pair ({i},{j}) for n={n}");
                    assert!(indices.insert(i), "layer pair shares index {i} (n={n})");
                    assert!(indices.insert(j), "layer pair shares index {j} (n={n})");
                }
            }

            // 0/1-Prinzip: alle 2^n Bit-Eingaben durchprobieren.
            for input in 0u32..(1u32 << n) {
                let mut arr: Vec<u8> = (0..n).map(|i| ((input >> i) & 1) as u8).collect();
                for layer in &layers {
                    for &(i, j) in layer {
                        // Absteigend: tausche, wenn arr[i] < arr[j].
                        if arr[i] < arr[j] {
                            arr.swap(i, j);
                        }
                    }
                }
                // Nachher muss die Folge absteigend sortiert sein.
                for k in 1..n {
                    assert!(
                        arr[k - 1] >= arr[k],
                        "n={n} input={input:0width$b} -> {arr:?} not sorted desc",
                        width = n.max(1)
                    );
                }
            }
        }
    }
}
