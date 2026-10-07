use std::ops::Range;

use super::Entry;

const DIGITS: usize = 16;
/// The low 8 key bytes are the seq.
const TILE_LAYER_DIGITS: Range<usize> = 8..DIGITS;

/// Stable LSD radix sort on the key bytes. When entries already arrive in seq order (the usual case for
/// a worker), only the tile/layer bytes are sorted, since stability keeps the seq order. Passes where
/// every entry shares the digit are skipped too: tile ids rarely use their top bytes.
pub(super) fn sort(entries: &mut Vec<Entry>, scratch: &mut Vec<Entry>, seq_sorted: bool) {
    let digits = if seq_sorted {
        TILE_LAYER_DIGITS
    } else {
        0..DIGITS
    };
    let n = entries.len();
    let mut counts = vec![[0usize; 256]; digits.len()];
    for entry in entries.iter() {
        for (digit, count) in digits.clone().zip(&mut counts) {
            count[usize::from(entry.digit(digit))] += 1;
        }
    }
    scratch.clear();
    scratch.resize(n, Entry::default());
    for (digit, count) in digits.zip(&counts) {
        if count.contains(&n) {
            continue;
        }
        let mut next = [0usize; 256];
        let mut sum = 0;
        for (slot, &c) in next.iter_mut().zip(count) {
            *slot = sum;
            sum += c;
        }
        for entry in entries.iter() {
            let slot = &mut next[usize::from(entry.digit(digit))];
            scratch[*slot] = *entry;
            *slot += 1;
        }
        std::mem::swap(entries, scratch);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sort::tests::random_keys;

    #[test]
    fn matches_stable_sort() {
        for (n, spread) in [(0, 1), (1, 1), (1000, 1 << 8), (5000, u64::MAX)] {
            let mut entries: Vec<_> = random_keys(n, spread)
                .enumerate()
                .map(|(i, key)| Entry::new(key, u32::try_from(i).unwrap(), 0))
                .collect();
            let mut expected = entries.clone();
            expected.sort_by_key(|e| e.key);
            let mut full = entries.clone();
            sort(&mut full, &mut Vec::new(), false);
            assert_eq!(full, expected, "n={n}");

            // Pre-sorting by seq makes the shortcut valid; the result must not change.
            entries.sort_by_key(|e| e.key[1]);
            sort(&mut entries, &mut Vec::new(), true);
            assert_eq!(entries, expected, "n={n} seq-sorted");
        }
    }
}
