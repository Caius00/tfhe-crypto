use rayon::prelude::*;
use tfhe::prelude::{CastInto, FheOrd, IfThenElse};
use tfhe::{FheInt128, FheInt64};

/// Berechnet die Summe aller Elemente homomorph.
/// Gibt FheInt128 zurück, um Overflow zu vermeiden
/// (z.B. 100 × i64::MAX würde i64 sprengen).
pub fn sum(list: &[FheInt64]) -> FheInt128 {
    list.par_iter()
        .map(|x| -> FheInt128 { x.clone().cast_into() })
        .reduce_with(|a, b| a + b)
        .expect("Liste darf nicht leer sein")
}

/// Gibt die Anzahl der Elemente zurück.
/// Kein Geheimnis: die Listenlänge ist dem Server bereits bekannt.
pub fn count(list: &[FheInt64]) -> usize {
    list.len()
}

/// Berechnet das Minimum der Liste homomorph.
pub fn min(list: &[FheInt64]) -> FheInt64 {
    list.par_iter()
        .cloned()
        .reduce_with(|a, b| a.gt(&b).if_then_else(&b, &a))
        .expect("Liste darf nicht leer sein")
}

/// Berechnet das Maximum der Liste homomorph.
pub fn max(list: &[FheInt64]) -> FheInt64 {
    list.par_iter()
        .cloned()
        .reduce_with(|a, b| a.gt(&b).if_then_else(&a, &b))
        .expect("Liste darf nicht leer sein")
}

/// Berechnet den Durchschnitt homomorph (Truncation toward zero).
/// Division durch einen Klartextwert (count) ist effizienter als FHE-Division.
pub fn average(list: &[FheInt64]) -> FheInt128 {
    let s = sum(list);
    let c = count(list) as i128;
    s / c
}

/// Tauscht zwei FheInt64-Werte homomorph so aus, dass danach gilt: result.0 <= result.1.
/// Da der Server den Vergleich nicht im Klartext auflösen kann, werden IMMER beide
/// Zweige berechnet – if_then_else wählt das Ergebnis homomorph aus.
fn compare_and_swap(a: FheInt64, b: FheInt64) -> (FheInt64, FheInt64) {
    let cond = a.gt(&b);
    let smaller = cond.if_then_else(&b, &a);
    let larger = cond.if_then_else(&a, &b);
    (smaller, larger)
}

/// Berechnet den Median der Liste mit **Odd-Even Transposition Sort**.
///
/// ### Wie Odd-Even Sort funktioniert
/// Das Verfahren arbeitet in n abwechselnden Phasen:
/// - **Gerade Phase** (phase % 2 == 0): Vergleiche Paare (0,1), (2,3), (4,5), …
/// - **Ungerade Phase** (phase % 2 == 1): Vergleiche Paare (1,2), (3,4), (5,6), …
///
/// Nach n Phasen ist die Liste vollständig sortiert. Entscheidend für die Performance:
/// Alle Vergleiche *innerhalb einer Phase* sind voneinander unabhängig und werden
/// hier mit Rayon **parallel** ausgeführt.
///
/// ### Rückgabewert
/// Bei ungerader Länge: das mittlere Element.
/// Bei gerader Länge: das untere der beiden mittleren Elemente (Lower Median).
pub fn median(list: &[FheInt64]) -> FheInt64 {
    let mut sorted = list.to_vec();
    let n = sorted.len();

    for phase in 0..n {
        let start = phase % 2;
        let pairs: Vec<usize> = (start..n.saturating_sub(1)).step_by(2).collect();

        let results: Vec<(FheInt64, FheInt64)> = pairs
            .par_iter()
            .map(|&i| compare_and_swap(sorted[i].clone(), sorted[i + 1].clone()))
            .collect();

        for (k, &i) in pairs.iter().enumerate() {
            sorted[i] = results[k].0.clone();
            sorted[i + 1] = results[k].1.clone();
        }
    }

    sorted[(n - 1) / 2].clone()
}
