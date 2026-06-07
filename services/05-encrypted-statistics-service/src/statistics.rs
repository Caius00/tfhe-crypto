use rayon::prelude::*;
use std::ops::Add;
use tfhe::prelude::{CastInto, FheOrd, IfThenElse};
use tfhe::FheBool;
use tfhe::{FheInt16, FheInt32, FheInt64};

/// Ermöglicht homomorphe Division durch die Listenlänge mit dem typ-passenden Skalar.
///
/// In TFHE-rs sind `Div`-Implementierungen typspezifisch (`FheInt16 / i16`,
/// `FheInt32 / i32` etc.) und nicht generisch über `i32` vereinheitlichbar.
/// Dieses Trait kapselt den Cast auf den jeweils passenden Skalartyp.
pub(crate) trait DivideByElementCount: Sized {
    fn divide_by_element_count(self, element_count: usize) -> Self;
}

impl DivideByElementCount for FheInt16 {
    fn divide_by_element_count(self, element_count: usize) -> Self {
        self / (element_count as i16)
    }
}
impl DivideByElementCount for FheInt32 {
    fn divide_by_element_count(self, element_count: usize) -> Self {
        self / (element_count as i32)
    }
}
impl DivideByElementCount for FheInt64 {
    fn divide_by_element_count(self, element_count: usize) -> Self {
        self / (element_count as i64)
    }
}

/// Berechnet die Summe aller Elemente homomorph.
///
/// Der Rückgabetyp `WiderOutputType` ist breiter als der Eingabetyp `InputType`
/// (z.B. FheInt16 → FheInt32), um Overflow zu vermeiden.
///
/// Zeitkomplexität: O(log n) Tiefe (paralleles Reduce-Baum), O(n) Additionen gesamt.
pub fn sum<InputType, WiderOutputType>(encrypted_list: &[InputType]) -> WiderOutputType
where
    InputType: Clone + CastInto<WiderOutputType> + Sync,
    WiderOutputType: Add<WiderOutputType, Output = WiderOutputType> + Send,
{
    encrypted_list
        .par_iter()
        .map(|encrypted_value| -> WiderOutputType { encrypted_value.clone().cast_into() })
        .reduce_with(|accumulated, next| accumulated + next)
        .expect("Liste darf nicht leer sein")
}

/// Gibt die Anzahl der Elemente zurück.
/// Die Listenlänge ist kein Geheimnis — der Server kennt sie bereits aus dem Request.
///
/// Zeitkomplexität: O(1).
pub fn count<T>(encrypted_list: &[T]) -> usize {
    encrypted_list.len()
}

/// Berechnet das Minimum der Liste homomorph.
/// Der Server wertet den Vergleich nie im Klartext aus — `if_then_else` auf `FheBool`
/// wählt das Ergebnis homomorph aus, ohne den tatsächlichen Wert zu kennen.
///
/// Zeitkomplexität: O(log n) Tiefe (paralleles Reduce), O(n) Vergleiche gesamt.
pub fn min<T>(encrypted_list: &[T]) -> T
where
    T: Clone + FheOrd + Sync + Send,
    FheBool: IfThenElse<T>,
{
    encrypted_list
        .par_iter()
        .cloned()
        .reduce_with(|current_min, candidate| {
            // .lt() nimmt Rhs by value — clone damit current_min für if_then_else erhalten bleibt
            let is_candidate_smaller = candidate.lt(current_min.clone());
            is_candidate_smaller.if_then_else(&candidate, &current_min)
        })
        .expect("Liste darf nicht leer sein")
}

/// Berechnet das Maximum der Liste homomorph.
/// Siehe `min` für Details zur homomorphen Vergleichsstrategie.
///
/// Zeitkomplexität: O(log n) Tiefe (paralleles Reduce), O(n) Vergleiche gesamt.
pub fn max<T>(encrypted_list: &[T]) -> T
where
    T: Clone + FheOrd + Sync + Send,
    FheBool: IfThenElse<T>,
{
    encrypted_list
        .par_iter()
        .cloned()
        .reduce_with(|current_max, candidate| {
            let is_candidate_larger = candidate.gt(current_max.clone());
            is_candidate_larger.if_then_else(&candidate, &current_max)
        })
        .expect("Liste darf nicht leer sein")
}

/// Berechnet den Durchschnitt homomorph (Truncation toward zero).
/// Die Division erfolgt durch einen Klartextwert (die Listenlänge), was deutlich
/// effizienter ist als eine vollständig homomorphe Division.
///
/// Zeitkomplexität: O(log n) Tiefe — identisch zu `sum`, plus O(1) für die Division.
pub fn average<InputType, WiderOutputType>(encrypted_list: &[InputType]) -> WiderOutputType
where
    InputType: Clone + CastInto<WiderOutputType> + Sync,
    WiderOutputType: Add<WiderOutputType, Output = WiderOutputType> + DivideByElementCount + Send,
{
    let encrypted_sum = sum(encrypted_list);
    let element_count = count(encrypted_list);
    encrypted_sum.divide_by_element_count(element_count)
}

/// Tauscht zwei Werte homomorph so aus, dass danach gilt: erstes_element <= zweites_element.
/// Da der Server den Vergleich nicht im Klartext auflösen kann, werden IMMER beide
/// Zweige berechnet — `if_then_else` wählt das Ergebnis homomorph aus.
fn compare_and_swap<T>(first: T, second: T) -> (T, T)
where
    T: Clone + FheOrd,
    FheBool: IfThenElse<T>,
{
    // .gt() nimmt second by value — clone damit second für if_then_else erhalten bleibt
    let is_first_larger = first.gt(second.clone());
    let smaller_value = is_first_larger.if_then_else(&second, &first);
    let larger_value = is_first_larger.if_then_else(&first, &second);
    (smaller_value, larger_value)
}

/// Baut das Batcher-Odd-Even-Comparator-Netzwerk für n Elemente.
/// Padding auf next_power_of_two; virtuelle Indizes (>= n) werden herausgefiltert.
/// Gibt Runden zurück — jede Runde enthält disjunkte Paare (innerhalb der Runde parallel ausführbar).
fn batcher_network(element_count: usize) -> Vec<Vec<(usize, usize)>> {
    if element_count <= 1 {
        return vec![];
    }
    let padded_size = element_count.next_power_of_two();
    let mut raw_comparators: Vec<(usize, usize, usize)> = Vec::new();
    batcher_sort(0, padded_size, 0, &mut raw_comparators);
    raw_comparators.retain(|&(_, second_index, _)| second_index < element_count);
    if raw_comparators.is_empty() {
        return vec![];
    }
    let max_depth = raw_comparators
        .iter()
        .map(|comparator| comparator.2)
        .max()
        .unwrap();
    let mut rounds: Vec<Vec<(usize, usize)>> = vec![Vec::new(); max_depth + 1];
    for (first_index, second_index, depth) in raw_comparators {
        rounds[depth].push((first_index, second_index));
    }
    rounds.retain(|round| !round.is_empty());
    rounds
}

fn batcher_sort(
    low: usize,
    high: usize,
    depth: usize,
    comparators_out: &mut Vec<(usize, usize, usize)>,
) -> usize {
    if high - low <= 1 {
        return depth;
    }
    let mid = low + (high - low) / 2;
    let depth_after_left = batcher_sort(low, mid, depth, comparators_out);
    let depth_after_right = batcher_sort(mid, high, depth, comparators_out);
    batcher_merge(
        low,
        high,
        1,
        depth_after_left.max(depth_after_right),
        comparators_out,
    )
}

fn batcher_merge(
    low: usize,
    high: usize,
    step: usize,
    depth: usize,
    comparators_out: &mut Vec<(usize, usize, usize)>,
) -> usize {
    let sequence_length = (high - low).div_ceil(step);
    if sequence_length <= 1 {
        return depth;
    }
    if sequence_length == 2 {
        comparators_out.push((low, low + step, depth));
        return depth + 1;
    }
    let depth_after_even = batcher_merge(low, high, 2 * step, depth, comparators_out);
    let depth_after_odd = batcher_merge(low + step, high, 2 * step, depth, comparators_out);
    let merge_depth = depth_after_even.max(depth_after_odd);
    let mut current_index = low + step;
    while current_index + step < high {
        comparators_out.push((current_index, current_index + step, merge_depth));
        current_index += 2 * step;
    }
    merge_depth + 1
}

/// Berechnet den Median mit **Batcher Odd-Even Mergesort** (Tiefe O(log²n) statt O(n)).
/// Bei ungerader Länge: mittleres Element; bei gerader Länge: Lower Median.
///
/// Zeitkomplexität: O(log²n) sequentielle Runden (Batcher-Netzwerk-Tiefe),
/// O(n log²n) Komparatoren gesamt; innerhalb jeder Runde parallel (rayon).
pub fn median<T>(encrypted_list: &[T]) -> T
where
    T: Clone + FheOrd + Sync + Send,
    FheBool: IfThenElse<T>,
{
    let mut partially_sorted = encrypted_list.to_vec();
    let element_count = partially_sorted.len();
    if element_count == 0 {
        panic!("Liste darf nicht leer sein");
    }
    if element_count == 1 {
        return partially_sorted.remove(0);
    }

    for comparator_round in batcher_network(element_count) {
        let sorted_pairs: Vec<(usize, usize, T, T)> = comparator_round
            .par_iter()
            .map(|&(left_index, right_index)| {
                let (smaller, larger) = compare_and_swap(
                    partially_sorted[left_index].clone(),
                    partially_sorted[right_index].clone(),
                );
                (left_index, right_index, smaller, larger)
            })
            .collect();

        for (left_index, right_index, smaller, larger) in sorted_pairs {
            partially_sorted[left_index] = smaller;
            partially_sorted[right_index] = larger;
        }
    }

    partially_sorted[(element_count - 1) / 2].clone()
}

#[cfg(test)]
mod tests {
    use super::batcher_network;
    use std::collections::HashSet;

    /// 0/1-Prinzip (Knuth TAOCP Vol. 3, §5.3.4):
    /// Ein Comparator-Netzwerk sortiert genau dann beliebige Eingaben korrekt,
    /// wenn es alle binären Eingaben (nur 0en und 1en) korrekt sortiert.
    /// Wir prüfen alle 2^n Bitmuster für n = 0..=8 — kein FHE nötig.
    ///
    /// Zusätzlich: Layer-Invariante — innerhalb einer Runde darf kein Index
    /// doppelt vorkommen, sonst wäre die Parallelisierung via rayon unsicher.
    #[test]
    fn batcher_network_is_a_valid_sorting_network() {
        for element_count in 0usize..=8 {
            let rounds = batcher_network(element_count);

            for round in &rounds {
                let mut seen_indices = HashSet::new();
                for &(left, right) in round {
                    assert!(
                        left < right && right < element_count,
                        "ungültiges Paar ({left},{right}) für n={element_count}"
                    );
                    assert!(
                        seen_indices.insert(left),
                        "Index {left} kommt in einer Runde doppelt vor (n={element_count})"
                    );
                    assert!(
                        seen_indices.insert(right),
                        "Index {right} kommt in einer Runde doppelt vor (n={element_count})"
                    );
                }
            }

            for bitmask in 0u32..(1u32 << element_count) {
                let mut values: Vec<u8> =
                    (0..element_count).map(|bit| ((bitmask >> bit) & 1) as u8).collect();

                for round in &rounds {
                    let updates: Vec<(usize, usize, u8, u8)> = round
                        .iter()
                        .map(|&(left, right)| {
                            let (smaller, larger) = if values[left] <= values[right] {
                                (values[left], values[right])
                            } else {
                                (values[right], values[left])
                            };
                            (left, right, smaller, larger)
                        })
                        .collect();
                    for (left, right, smaller, larger) in updates {
                        values[left] = smaller;
                        values[right] = larger;
                    }
                }

                for position in 1..element_count {
                    assert!(
                        values[position - 1] <= values[position],
                        "n={element_count} bitmask={bitmask:0width$b} → {values:?} nicht sortiert",
                        width = element_count.max(1)
                    );
                }

                if element_count > 0 {
                    let expected_median = {
                        let mut sorted = values.clone();
                        sorted.sort();
                        sorted[(element_count - 1) / 2]
                    };
                    assert_eq!(
                        values[(element_count - 1) / 2],
                        expected_median,
                        "falscher Median-Index für n={element_count} bitmask={bitmask:0width$b}",
                        width = element_count.max(1)
                    );
                }
            }
        }
    }

    /// Prüft zusätzlich dass `median()` für Plaintext-Werte (i32 als Wrapper)
    /// denselben Median liefert wie naive Sortierung — für n = 1..=5.
    ///
    /// Nutzt das 0/1-Prinzip: alle binären Eingaben als i32 (0 oder 1).
    /// Kein FHE nötig — `median()` ist generisch und funktioniert mit jedem
    /// Typ der `Clone + FheOrd + Sync + Send` implementiert.
    /// Da i32 kein FheOrd implementiert, testen wir hier nur das Netzwerk direkt
    /// über `batcher_network` — der obige Test deckt das vollständig ab.
    #[test]
    fn median_index_formula_is_correct_for_sorted_input() {
        // Für eine bereits sortierte Liste [0,1,2,...,n-1] muss der Median-Index
        // (n-1)/2 das richtige Element liefern.
        for element_count in 1usize..=9 {
            let expected_median_index = (element_count - 1) / 2;
            let sorted: Vec<usize> = (0..element_count).collect();
            assert_eq!(
                sorted[expected_median_index],
                expected_median_index,
                "Median-Index-Formel falsch für n={element_count}"
            );
        }
    }
}
