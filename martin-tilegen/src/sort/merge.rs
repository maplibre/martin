use std::fs::File;
use std::io::{BufReader, Read as _, Seek as _};

use integer_encoding::VarIntReader as _;

use super::Run;
use crate::{SortKey, TileGenResult};

/// Streams the records of several runs in key order. Equal keys come out in run order,
/// because the cursors are kept in run-index order and ties are broken by cursor position.
pub struct Merger {
    cursors: Vec<Cursor>,
    tree: LoserTree,
    /// The cursor whose record was returned last; it advances on the next call, so the record can be borrowed.
    returned: Option<usize>,
}

impl Merger {
    pub(super) fn new(runs: Vec<Run>, read_buffer_bytes: usize) -> TileGenResult<Self> {
        let cursors = runs
            .into_iter()
            .map(|run| Cursor::new(run, read_buffer_bytes))
            .collect::<TileGenResult<Vec<_>>>()?;
        let tree = LoserTree::new(cursors.len(), |a, b| before(&cursors, a, b));
        Ok(Self {
            cursors,
            tree,
            returned: None,
        })
    }

    #[expect(
        clippy::should_implement_trait,
        reason = "records are borrowed, which `Iterator` cannot express"
    )]
    pub fn next(&mut self) -> TileGenResult<Option<(SortKey, &[u8])>> {
        if let Some(last) = self.returned.take() {
            self.cursors[last].advance()?;
            let cursors = &self.cursors;
            self.tree.replay(last, |a, b| before(cursors, a, b));
        }
        let Some(winner) = self.tree.winner() else {
            return Ok(None);
        };
        let cursor = &self.cursors[winner];
        Ok(cursor.head.map(|key| {
            self.returned = Some(winner);
            (key, cursor.record.as_slice())
        }))
    }
}

/// Exhausted cursors sort last; ties go to the earlier run.
fn before(cursors: &[Cursor], a: usize, b: usize) -> bool {
    match (cursors[a].head, cursors[b].head) {
        (Some(x), Some(y)) => (x, a) < (y, b),
        (x, _) => x.is_some(),
    }
}

struct Cursor {
    reader: BufReader<File>,
    remaining: u64,
    head: Option<SortKey>,
    record: Vec<u8>,
}

impl Cursor {
    fn new(run: Run, read_buffer_bytes: usize) -> TileGenResult<Self> {
        let mut file = run.file;
        file.rewind()?;
        let mut cursor = Self {
            reader: BufReader::with_capacity(read_buffer_bytes, file),
            remaining: run.records,
            head: None,
            record: Vec::new(),
        };
        cursor.advance()?;
        Ok(cursor)
    }

    /// The record count is known, so any EOF while reading is a truncated run, not the end.
    fn advance(&mut self) -> TileGenResult<()> {
        if self.remaining == 0 {
            self.head = None;
            return Ok(());
        }
        self.remaining -= 1;
        let mut key = [0; SortKey::ENCODED_LEN];
        self.reader.read_exact(&mut key)?;
        let len: usize = self.reader.read_varint()?;
        self.record.resize(len, 0);
        self.reader.read_exact(&mut self.record)?;
        self.head = Some(SortKey::from_bytes(key));
        Ok(())
    }
}

/// Tournament tree over `k` leaves: `nodes[0]` is the winner, `nodes[1..k]` the losers of each match,
/// so replacing the winner costs one comparison per level instead of a heap's two.
struct LoserTree {
    nodes: Vec<usize>,
}

impl LoserTree {
    const EMPTY: usize = usize::MAX;

    fn new(k: usize, before: impl Fn(usize, usize) -> bool) -> Self {
        let mut tree = Self {
            nodes: vec![Self::EMPTY; k],
        };
        for leaf in 0..k {
            tree.replay(leaf, &before);
        }
        tree
    }

    fn winner(&self) -> Option<usize> {
        self.nodes.first().copied()
    }

    /// Re-runs the matches from `leaf` to the root. While building, the first leaf to reach an empty
    /// match waits there for its opponent.
    fn replay(&mut self, leaf: usize, before: impl Fn(usize, usize) -> bool) {
        let k = self.nodes.len();
        let mut winner = leaf;
        let mut node = usize::midpoint(leaf, k);
        while node > 0 {
            let other = self.nodes[node];
            if other == Self::EMPTY {
                self.nodes[node] = winner;
                return;
            }
            if before(other, winner) {
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
            let key = |heads: &[Vec<u32>], i: usize| heads[i].last().copied();
            let before =
                |heads: &[Vec<u32>], a: usize, b: usize| match (key(heads, a), key(heads, b)) {
                    (Some(x), Some(y)) => (x, a) < (y, b),
                    (x, _) => x.is_some(),
                };
            let mut tree = LoserTree::new(k as usize, |a, b| before(&heads, a, b));
            let mut out = Vec::new();
            while let Some(w) = tree.winner().filter(|&w| key(&heads, w).is_some()) {
                out.push((heads[w].pop().unwrap(), w));
                tree.replay(w, |a, b| before(&heads, a, b));
            }
            let mut expected = out.clone();
            expected.sort_unstable();
            assert_eq!(out, expected, "k={k}");
            assert_eq!(out.len(), 5 * k as usize);
        }
    }
}
