use super::run::{Run, RunReader};
use crate::{SortKey, TileGenResult};

/// Streams the records of several runs in key order. Equal keys come out in run order,
/// because the readers are kept in run-index order and ties are broken by reader position.
pub struct Merger {
    readers: Vec<RunReader>,
    tree: LoserTree,
    /// Advanced on the next call, so the record it returned can be borrowed until then.
    last_yielded: Option<usize>,
}

impl Merger {
    pub(super) fn new(runs: Vec<Run>, read_buffer_bytes: usize) -> TileGenResult<Self> {
        let readers = runs
            .into_iter()
            .map(|run| RunReader::open(run, read_buffer_bytes))
            .collect::<TileGenResult<Vec<_>>>()?;
        let tree = LoserTree::new(readers.len(), |a, b| precedes(&readers, a, b));
        Ok(Self {
            readers,
            tree,
            last_yielded: None,
        })
    }

    pub fn next_record(&mut self) -> TileGenResult<Option<(SortKey, &[u8])>> {
        if let Some(last) = self.last_yielded.take() {
            self.readers[last].advance()?;
            let readers = &self.readers;
            self.tree.replay(last, |a, b| precedes(readers, a, b));
        }
        let Some(winner) = self.tree.winner() else {
            return Ok(None);
        };
        let reader = &self.readers[winner];
        Ok(reader.head().map(|key| {
            self.last_yielded = Some(winner);
            (key, reader.record())
        }))
    }
}

fn precedes(readers: &[RunReader], a: usize, b: usize) -> bool {
    head_precedes(readers[a].head(), a, readers[b].head(), b)
}

/// Exhausted heads sort last; ties go to the earlier run.
fn head_precedes<K: Ord>(head_a: Option<K>, a: usize, head_b: Option<K>, b: usize) -> bool {
    match (head_a, head_b) {
        (Some(x), Some(y)) => (x, a) < (y, b),
        (x, _) => x.is_some(),
    }
}

/// Tournament tree over `k` leaves: `nodes[0]` is the winner, `nodes[1..k]` the losers of each match,
/// so replacing the winner costs one comparison per level instead of a heap's two.
struct LoserTree {
    nodes: Vec<usize>,
}

impl LoserTree {
    const EMPTY: usize = usize::MAX;

    fn new(k: usize, precedes: impl Fn(usize, usize) -> bool) -> Self {
        let mut tree = Self {
            nodes: vec![Self::EMPTY; k],
        };
        for leaf in 0..k {
            tree.replay(leaf, &precedes);
        }
        tree
    }

    fn winner(&self) -> Option<usize> {
        self.nodes.first().copied()
    }

    /// While building, the first leaf to reach an empty match waits there for its opponent.
    fn replay(&mut self, leaf: usize, precedes: impl Fn(usize, usize) -> bool) {
        let k = self.nodes.len();
        let mut winner = leaf;
        let mut node = usize::midpoint(leaf, k);
        while node > 0 {
            let other = self.nodes[node];
            if other == Self::EMPTY {
                self.nodes[node] = winner;
                return;
            }
            if precedes(other, winner) {
                self.nodes[node] = winner;
                winner = other;
            }
            node /= 2;
        }
        self.nodes[0] = winner;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loser_tree_yields_sorted_order_for_any_width() {
        for k in 1..=9 {
            let mut heads: Vec<Vec<u32>> = (0..k)
                .map(|i| (0..5).map(|j| j * 3 + i % 3).rev().collect())
                .collect();
            let head = |heads: &[Vec<u32>], i: usize| heads[i].last().copied();
            let precedes = |heads: &[Vec<u32>], a: usize, b: usize| {
                head_precedes(head(heads, a), a, head(heads, b), b)
            };
            let mut tree = LoserTree::new(k as usize, |a, b| precedes(&heads, a, b));
            let mut out = Vec::new();
            while let Some(w) = tree.winner().filter(|&w| head(&heads, w).is_some()) {
                out.push((heads[w].pop().unwrap(), w));
                tree.replay(w, |a, b| precedes(&heads, a, b));
            }
            let mut expected = out.clone();
            expected.sort_unstable();
            assert_eq!(out, expected, "k={k}");
            assert_eq!(out.len(), 5 * k as usize);
        }
    }
}
