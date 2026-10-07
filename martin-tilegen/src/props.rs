//! Feature properties as they travel through the engine: interned keys and borrowed typed values.
//! Every property is dynamic: a tile's columns are whatever keys its features carry.

use std::cmp::Ordering;
use std::num::NonZeroU32;

use lasso::{Key as _, RodeoResolver, Spur, ThreadedRodeo};
use mlt_core::{PropKind, PropValueRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(pub(crate) u32);

impl KeyId {
    fn from_spur(spur: Spur) -> Self {
        Self(index_u32(spur.into_usize()))
    }

    fn spur(self) -> Spur {
        Spur::try_from_usize(self.0 as usize).expect("key ids come from a Spur")
    }
}

/// The id of the `n`th key declared up front: a source can use column positions without interning.
impl From<u32> for KeyId {
    fn from(n: u32) -> Self {
        Self(n)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Bool(bool),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(String),
}

impl Prop {
    #[must_use]
    pub fn as_ref(&self) -> PropValueRef<'_> {
        match self {
            Self::Bool(v) => PropValueRef::Bool(*v),
            Self::I64(v) => PropValueRef::I64(*v),
            Self::F32(v) => PropValueRef::F32(*v),
            Self::F64(v) => PropValueRef::F64(*v),
            Self::Str(v) => PropValueRef::Str(v),
        }
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
        KeyId::from_spur(self.keys.get_or_intern(name))
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<KeyId> {
        self.keys.get(name).map(KeyId::from_spur)
    }

    #[must_use]
    pub fn name(&self, id: KeyId) -> Option<String> {
        Spur::try_from_usize(id.0 as usize)
            .and_then(|spur| self.keys.try_resolve(&spur))
            .map(str::to_owned)
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
    use super::*;

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
        let names = interner.freeze();
        for (i, id) in ids[0].iter().enumerate() {
            assert_eq!(names.name(*id), format!("k{i}"));
        }
    }
}
