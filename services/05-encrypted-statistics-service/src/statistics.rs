use rayon::prelude::*;
use tfhe::prelude::{CastInto, FheOrd, IfThenElse};
use tfhe::{FheInt32, FheInt64};

/// Berechnet die Summe aller Elemente homomorph.
/// Gibt FheInt64 zurück, um Overflow zu vermeiden
/// (z.B. 100 × i32::MAX würde i32 sprengen).
pub fn sum(list: &[FheInt32]) -> FheInt64 {
    list.par_iter()
        .map(|x| -> FheInt64 { x.clone().cast_into() })
        .reduce_with(|a, b| a + b)
        .expect("Liste darf nicht leer sein")
}

/// Gibt die Anzahl der Elemente zurück.
/// Kein Geheimnis: die Listenlänge ist dem Server bereits bekannt.
pub fn count(list: &[FheInt32]) -> usize {
    list.len()
}

/// Berechnet das Minimum der Liste homomorph.
pub fn min(list: &[FheInt32]) -> FheInt32 {
    list.par_iter()
        .cloned()
        .reduce_with(|a, b| a.gt(&b).if_then_else(&b, &a))
        .expect("Liste darf nicht leer sein")
}

/// Berechnet das Maximum der Liste homomorph.
pub fn max(list: &[FheInt32]) -> FheInt32 {
    list.par_iter()
        .cloned()
        .reduce_with(|a, b| a.gt(&b).if_then_else(&a, &b))
        .expect("Liste darf nicht leer sein")
}

/// Berechnet den Durchschnitt homomorph (Truncation toward zero).
/// Division durch einen Klartextwert (count) ist effizienter als FHE-Division.
pub fn average(list: &[FheInt32]) -> FheInt64 {
    let s = sum(list);
    let c = count(list) as i64;
    s / c
}

/// Tauscht zwei FheInt32-Werte homomorph so aus, dass danach gilt: result.0 <= result.1.
/// Da der Server den Vergleich nicht im Klartext auflösen kann, werden IMMER beide
/// Zweige berechnet – if_then_else wählt das Ergebnis homomorph aus.
fn compare_and_swap(a: FheInt32, b: FheInt32) -> (FheInt32, FheInt32) {
    let cond = a.gt(&b);
    let smaller = cond.if_then_else(&b, &a);
    let larger = cond.if_then_else(&a, &b);
    (smaller, larger)
}

/// Baut das Batcher-Comparator-Netzwerk für n Elemente.
/// Padding auf next_power_of_two; virtuelle Indizes (>= n) werden herausgefiltert.
/// Gibt Runden zurück — jede Runde enthält disjunkte Paare (innerhalb der Runde parallel ausführbar).
fn batcher_network(n: usize) -> Vec<Vec<(usize, usize)>> {
    if n <= 1 {
        return vec![];
    }
    let n2 = n.next_power_of_two();
    let mut raw: Vec<(usize, usize, usize)> = Vec::new();
    batcher_sort(0, n2, 0, &mut raw);
    // Virtuelle Slots (>= n) repräsentieren +∞; Comparatoren die sie berühren sind no-ops.
    raw.retain(|&(_, j, _)| j < n);
    if raw.is_empty() {
        return vec![];
    }
    let max_d = raw.iter().map(|c| c.2).max().unwrap();
    let mut rounds: Vec<Vec<(usize, usize)>> = vec![Vec::new(); max_d + 1];
    for (i, j, d) in raw {
        rounds[d].push((i, j));
    }
    rounds.retain(|r| !r.is_empty());
    rounds
}

fn batcher_sort(lo: usize, hi: usize, depth: usize, out: &mut Vec<(usize, usize, usize)>) -> usize {
    if hi - lo <= 1 {
        return depth;
    }
    let mid = lo + (hi - lo) / 2;
    // Beide Hälften können parallel sortiert werden (gleiche Starttiefe).
    let d1 = batcher_sort(lo, mid, depth, out);
    let d2 = batcher_sort(mid, hi, depth, out);
    batcher_merge(lo, hi, 1, d1.max(d2), out)
}

fn batcher_merge(
    lo: usize,
    hi: usize,
    step: usize,
    depth: usize,
    out: &mut Vec<(usize, usize, usize)>,
) -> usize {
    let len = (hi - lo).div_ceil(step);
    if len <= 1 {
        return depth;
    }
    if len == 2 {
        out.push((lo, lo + step, depth));
        return depth + 1;
    }
    // Gerade und ungerade Teil-Sequenzen können parallel gemergt werden.
    let d1 = batcher_merge(lo, hi, 2 * step, depth, out);
    let d2 = batcher_merge(lo + step, hi, 2 * step, depth, out);
    let d = d1.max(d2);
    let mut i = lo + step;
    while i + step < hi {
        out.push((i, i + step, d));
        i += 2 * step;
    }
    d + 1
}

/// Berechnet den Median mit **Batcher Odd-Even Mergesort** (Tiefe O(log²n) statt O(n)).
/// Bei ungerader Länge: mittleres Element; bei gerader Länge: Lower Median.
pub fn median(list: &[FheInt32]) -> FheInt32 {
    let mut sorted = list.to_vec();
    let n = sorted.len();
    if n == 0 {
        panic!("Liste darf nicht leer sein");
    }
    if n == 1 {
        return sorted.remove(0);
    }

    for round in batcher_network(n) {
        let results: Vec<(usize, usize, FheInt32, FheInt32)> = round
            .par_iter()
            .map(|&(i, j)| {
                let (lo, hi) = compare_and_swap(sorted[i].clone(), sorted[j].clone());
                (i, j, lo, hi)
            })
            .collect();
        for (i, j, lo, hi) in results {
            sorted[i] = lo;
            sorted[j] = hi;
        }
    }

    sorted[(n - 1) / 2].clone()
}
