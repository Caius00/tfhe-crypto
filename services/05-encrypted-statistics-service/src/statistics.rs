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
pub fn count<T>(encrypted_list: &[T]) -> usize {
    encrypted_list.len()
}

/// Berechnet das Minimum der Liste homomorph.
/// Der Server wertet den Vergleich nie im Klartext aus — `if_then_else` auf `FheBool`
/// wählt das Ergebnis homomorph aus, ohne den tatsächlichen Wert zu kennen.
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
