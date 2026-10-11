//! Feature properties as they travel through the engine: interned keys and borrowed typed values.
//! Every property is dynamic: a tile's columns are whatever keys its features carry.

use std::cmp::Ordering;
use std::num::NonZeroU32;

use lasso::{Key as _, RodeoResolver, Spur, ThreadedRodeo};
use mlt_core::{PropKind, PropValueRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(pub(crate) u32);

impl KeyId {
    fn spur(self) -> Spur {
        Spur::try_from_usize(self.0 as usize).expect("key ids come from a Spur")
    }
}

fn index_u32(len: usize) -> u32 {
    u32::try_from(len).expect("fewer than 2^32 property keys or columns")
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

/// Property keys of one layer, shared by all workers. Keys the source declares up front get the first ids
/// in declaration order, which is their column order in every tile; keys found later sort by name.
pub struct KeyInterner {
    known: u32,
    keys: ThreadedRodeo,
}

impl KeyInterner {
    pub fn new<S: AsRef<str>>(known: impl IntoIterator<Item = S>) -> Self {
        let keys = ThreadedRodeo::new();
        for name in known {
            keys.get_or_intern(name.as_ref());
        }
        Self {
            known: index_u32(keys.len()),
            keys,
        }
    }

    pub fn intern(&self, name: &str) -> KeyId {
        KeyId(index_u32(self.keys.get_or_intern(name).into_usize()))
    }

    /// Call once no more keys can appear, i.e. after rendering.
    #[must_use]
    pub fn freeze(self) -> KeyNames {
        KeyNames {
            known: self.known,
            names: self.keys.into_resolver(),
        }
    }
}

pub struct KeyNames {
    known: u32,
    names: RodeoResolver,
}

impl KeyNames {
    #[must_use]
    pub fn name(&self, id: KeyId) -> &str {
        self.names.resolve(&id.spur())
    }

    fn column_order(&self, a: KeyId, b: KeyId) -> Ordering {
        match (a.0 < self.known, b.0 < self.known) {
            (true, true) => a.cmp(&b),
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            (false, false) => self.name(a).cmp(self.name(b)),
        }
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
            self.column_of_key[column.key.0 as usize] = ColumnIndex::default();
        }
        self.columns.clear();
    }

    fn column_index_mut(&mut self, key: KeyId) -> &mut ColumnIndex {
        let idx = key.0 as usize;
        if idx >= self.column_of_key.len() {
            self.column_of_key.resize(idx + 1, ColumnIndex::default());
        }
        &mut self.column_of_key[idx]
    }

    pub(crate) fn add(&mut self, key: KeyId, value: PropValueRef<'_>) {
        let idx = if let Some(idx) = self.column_index_mut(key).get() {
            idx
        } else {
            self.columns.push(Column {
                key,
                kind: value.kind(),
                has_negative: false,
            });
            let idx = self.columns.len() - 1;
            *self.column_index_mut(key) = ColumnIndex::new(idx);
            idx
        };
        let column = &mut self.columns[idx];
        column.kind = column.kind.merge(value.kind());
        column.has_negative |= matches!(value, PropValueRef::I64(v) if v < 0);
    }

    /// Final `(key, kind)` per column in column order; the index maps keys to positions in it.
    pub(crate) fn finish(&mut self, names: &KeyNames) -> Vec<(KeyId, PropKind)> {
        self.columns
            .sort_by(|a, b| names.column_order(a.key, b.key));
        for (pos, column) in self.columns.iter().enumerate() {
            self.column_of_key[column.key.0 as usize] = ColumnIndex::new(pos);
        }
        self.columns
            .iter()
            .map(|c| {
                let unsigned = c.kind == PropKind::I64 && !c.has_negative;
                (c.key, if unsigned { PropKind::U64 } else { c.kind })
            })
            .collect()
    }

    /// Column position of `key`, valid after [`finish`](Self::finish).
    pub(crate) fn position(&self, key: KeyId) -> Option<usize> {
        self.column_of_key.get(key.0 as usize)?.get()
    }
}

#[cfg(test)]
mod tests {
    use mlt_core::PropValue;

    use super::*;

    #[test]
    fn known_keys_come_first_in_declaration_order() {
        let interner = KeyInterner::new(["zeta", "alpha"]);
        let dynamic_b = interner.intern("b");
        let dynamic_a = interner.intern("a");
        assert_eq!(interner.intern("zeta"), KeyId(0));
        let names = interner.freeze();
        let mut columns = TileColumns::default();
        for key in [dynamic_b, KeyId(1), dynamic_a, KeyId(0)] {
            columns.add(key, PropValueRef::I64(1));
        }
        let order: Vec<_> = columns
            .finish(&names)
            .iter()
            .map(|(k, _)| names.name(*k).to_owned())
            .collect();
        assert_eq!(order, ["zeta", "alpha", "a", "b"]);
    }

    #[test]
    fn kinds_merge_like_mlt_core() {
        let names = KeyInterner::new(["n", "f", "s", "u", "m"]).freeze();
        let mut columns = TileColumns::default();
        for (key, value) in [
            (0, PropValueRef::I64(-1)),
            (0, PropValueRef::I64(5)),
            (1, PropValueRef::F32(1.5)),
            (1, PropValueRef::F64(2.5)),
            (2, PropValueRef::I64(1)),
            (2, PropValueRef::Bool(true)),
            (3, PropValueRef::I64(7)),
            (4, PropValueRef::I64(3)),
            (4, PropValueRef::F64(1.5)),
        ] {
            columns.add(KeyId(key), value);
        }
        let kinds: Vec<_> = columns.finish(&names).into_iter().map(|(_, k)| k).collect();
        assert_eq!(
            kinds,
            [
                PropKind::I64,
                PropKind::F64,
                PropKind::Str,
                PropKind::U64,
                PropKind::F64
            ]
        );
        assert_eq!(
            PropValueRef::I64(3).coerce(PropKind::F64),
            PropValue::F64(Some(3.0))
        );
        assert_eq!(
            PropValueRef::I64(1).coerce(PropKind::Str),
            PropValue::Str(Some("1".to_owned()))
        );
        assert_eq!(
            PropValueRef::F32(1.5).coerce(PropKind::F64),
            PropValue::F64(Some(1.5))
        );
    }

    #[test]
    fn interning_is_thread_safe() {
        let interner = KeyInterner::new::<&str>([]);
        let ids: Vec<Vec<KeyId>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        (0..100)
                            .map(|i| interner.intern(&format!("k{i}")))
                            .collect()
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(ids.windows(2).all(|w| w[0] == w[1]));
        assert_eq!(interner.freeze().names.len(), 100);
    }
}
