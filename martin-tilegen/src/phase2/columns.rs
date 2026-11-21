//! The columns of one layer in one tile, built from the keys its features carry.

use std::cmp::Ordering;
use std::num::NonZeroU32;

use mlt_core::{PropKind, PropValueRef};

use crate::props::{KeyId, KeyNames};

fn index_u32(len: usize) -> u32 {
    u32::try_from(len).expect("fewer than 2^32 columns")
}

/// Position of a key's column among the columns of one layer in one tile (!= [`KeyId`] the key for the whole layer).
///
/// `None` while no feature of the tile carries the key.
#[derive(Clone, Copy, Default)]
struct ColumnIndex(Option<NonZeroU32>);

impl ColumnIndex {
    fn new(position: usize) -> Self {
        Self(NonZeroU32::new(index_u32(position + 1)))
    }

    fn get(self) -> Option<usize> {
        self.0.map(|n| n.get() as usize - 1)
    }
}

/// The columns of one layer in one tile, built from the values its features carry. Reused across tiles.
#[derive(Default)]
pub(crate) struct TileColumns {
    column_of_key: Vec<ColumnIndex>,
    columns: Vec<Column>,
}

struct Column {
    key: KeyId,
    kind: PropKind,
    has_negative: bool,
}

impl TileColumns {
    pub(crate) fn clear(&mut self) {
        for column in &self.columns {
            self.column_of_key[column.key.index()] = ColumnIndex::default();
        }
        self.columns.clear();
    }

    fn column_index_mut(&mut self, key: KeyId) -> &mut ColumnIndex {
        let idx = key.index();
        if idx >= self.column_of_key.len() {
            self.column_of_key.resize(idx + 1, ColumnIndex::default());
        }
        &mut self.column_of_key[idx]
    }

    pub(crate) fn add(&mut self, key: KeyId, value: PropValueRef<'_>) {
        let idx = self.add_kind(key, value.kind());
        self.columns[idx].has_negative |= matches!(value, PropValueRef::I64(v) if v < 0);
    }

    /// Adds a column of `kind` for the scalar documents of `key`, whatever the tile's values are.
    #[cfg(feature = "unstable-mlt-v2")]
    pub(crate) fn add_document(&mut self, key: KeyId, kind: PropKind) {
        let idx = self.add_kind(key, kind);
        self.columns[idx].has_negative = true;
    }

    fn add_kind(&mut self, key: KeyId, kind: PropKind) -> usize {
        let idx = if let Some(idx) = self.column_index_mut(key).get() {
            idx
        } else {
            self.columns.push(Column {
                key,
                kind,
                has_negative: false,
            });
            let idx = self.columns.len() - 1;
            *self.column_index_mut(key) = ColumnIndex::new(idx);
            idx
        };
        let column = &mut self.columns[idx];
        column.kind = column.kind.merge(kind);
        idx
    }

    /// Fills `schema` with the final `(key, kind)` per column in column order; the index maps keys to
    /// positions in it. A key whose kinds `names` tallied gets that kind in every tile.
    pub(crate) fn finish(&mut self, names: &KeyNames, schema: &mut Vec<(KeyId, PropKind)>) {
        self.columns.sort_by(|a, b| column_order(names, a.key, b.key));
        for (pos, column) in self.columns.iter().enumerate() {
            self.column_of_key[column.key.index()] = ColumnIndex::new(pos);
        }
        schema.clear();
        schema.extend(self.columns.iter().map(|c| {
            let (kind, has_negative) = match names.kind(c.key) {
                #[cfg(feature = "unstable-mlt-v2")]
                _ if names.document(c.key).is_some() => (c.kind, c.has_negative),
                Some(tallied) => (c.kind.merge(tallied.merged()), c.has_negative || tallied.has_negative),
                None => (c.kind, c.has_negative),
            };
            let unsigned = kind == PropKind::I64 && !has_negative;
            (c.key, if unsigned { PropKind::U64 } else { kind })
        }));
    }

    /// Column position of `key`, valid after [`finish`](Self::finish).
    pub(crate) fn position(&self, key: KeyId) -> Option<usize> {
        self.column_of_key.get(key.index())?.get()
    }
}

pub(super) fn column_order(names: &KeyNames, a: KeyId, b: KeyId) -> Ordering {
    match (names.is_declared(a), names.is_declared(b)) {
        (true, true) => a.cmp(&b),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => names.name(a).cmp(names.name(b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::KeyInterner;

    #[test]
    fn known_keys_come_first_in_declaration_order() {
        let interner = KeyInterner::new(["zeta", "alpha"]);
        let dynamic_b = interner.intern("b");
        let dynamic_a = interner.intern("a");
        assert_eq!(interner.intern("zeta"), KeyId::new(0));
        let names = interner.freeze();
        let mut columns = TileColumns::default();
        for key in [dynamic_b, KeyId::new(1), dynamic_a, KeyId::new(0)] {
            columns.add(key, PropValueRef::I64(1));
        }
        let mut schema = Vec::new();
        columns.finish(&names, &mut schema);
        let order: Vec<_> = schema.iter().map(|(k, _)| names.name(*k)).collect();
        assert_eq!(order, ["zeta", "alpha", "a", "b"]);
    }
}
