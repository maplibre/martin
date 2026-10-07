//! Feature properties as they travel through the engine: interned keys and borrowed typed values.
//! Every property is dynamic: a tile's columns are whatever keys its features carry.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

use mlt_core::{PropKind, PropValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(pub(crate) u32);

/// The id of the `n`th key declared up front: a source can use column positions without interning.
impl From<u32> for KeyId {
    fn from(n: u32) -> Self {
        Self(n)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Prop<S = String> {
    Bool(bool),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(S),
}

pub type PropRef<'a> = Prop<&'a str>;

impl<S: AsRef<str>> Prop<S> {
    #[must_use]
    pub fn as_ref(&self) -> PropRef<'_> {
        match self {
            Self::Bool(v) => Prop::Bool(*v),
            Self::I64(v) => Prop::I64(*v),
            Self::F32(v) => Prop::F32(*v),
            Self::F64(v) => Prop::F64(*v),
            Self::Str(v) => Prop::Str(v.as_ref()),
        }
    }
}

impl PropRef<'_> {
    fn kind(self) -> PropKind {
        match self {
            Self::Bool(_) => PropKind::Bool,
            Self::I64(_) => PropKind::I64,
            Self::F32(_) => PropKind::F32,
            Self::F64(_) => PropKind::F64,
            Self::Str(_) => PropKind::Str,
        }
    }

    /// The value in the column type a tile settled on (see [`TileColumns`]).
    pub(crate) fn to_value(self, kind: PropKind) -> PropValue {
        match (kind, self) {
            (PropKind::Bool, Self::Bool(v)) => PropValue::Bool(Some(v)),
            (PropKind::I64, Self::I64(v)) => PropValue::I64(Some(v)),
            (PropKind::U64, Self::I64(v)) => PropValue::U64(u64::try_from(v).ok()),
            (PropKind::F32, Self::F32(v)) => PropValue::F32(Some(v)),
            (PropKind::F64, Self::F32(v)) => PropValue::F64(Some(f64::from(v))),
            (PropKind::F64, Self::F64(v)) => PropValue::F64(Some(v)),
            #[expect(
                clippy::cast_precision_loss,
                reason = "as mlt-core converts integers in float columns"
            )]
            (PropKind::F64, Self::I64(v)) => PropValue::F64(Some(v as f64)),
            (_, other) => PropValue::Str(Some(other.to_text())),
        }
    }

    fn to_text(self) -> String {
        match self {
            Self::Str(v) => v.to_owned(),
            Self::Bool(v) => v.to_string(),
            Self::I64(v) => v.to_string(),
            Self::F32(v) => v.to_string(),
            Self::F64(v) => v.to_string(),
        }
    }
}

fn index_u32(len: usize) -> u32 {
    u32::try_from(len).expect("fewer than 2^32 property keys or columns")
}

/// Property keys of one layer, shared by all workers. Keys the source declares up front get the first ids
/// in declaration order, which is their column order in every tile; keys found later sort by name.
pub struct KeyInterner {
    known: u32,
    keys: RwLock<Keys>,
}

#[derive(Default)]
struct Keys {
    ids: HashMap<String, KeyId>,
    names: Vec<String>,
}

impl KeyInterner {
    pub fn new<S: AsRef<str>>(known: impl IntoIterator<Item = S>) -> Self {
        let mut interner = Self {
            known: 0,
            keys: RwLock::default(),
        };
        for name in known {
            interner.intern(name.as_ref());
        }
        interner.known = interner.len();
        interner
    }

    pub fn intern(&self, name: &str) -> KeyId {
        if let Some(id) = self.get(name) {
            return id;
        }
        let mut keys = self.keys.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(&id) = keys.ids.get(name) {
            return id;
        }
        let id = KeyId(index_u32(keys.names.len()));
        keys.names.push(name.to_owned());
        keys.ids.insert(name.to_owned(), id);
        id
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<KeyId> {
        self.keys
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .ids
            .get(name)
            .copied()
    }

    #[must_use]
    pub fn name(&self, id: KeyId) -> Option<String> {
        self.keys
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .names
            .get(id.0 as usize)
            .cloned()
    }

    fn len(&self) -> u32 {
        let keys = self.keys.read().unwrap_or_else(PoisonError::into_inner);
        index_u32(keys.names.len())
    }

    /// Call once no more keys can appear, i.e. after rendering.
    #[must_use]
    pub fn freeze(self) -> KeyNames {
        KeyNames {
            known: self.known,
            names: self
                .keys
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner)
                .names,
        }
    }
}

pub struct KeyNames {
    known: u32,
    names: Vec<String>,
}

impl KeyNames {
    #[must_use]
    pub fn name(&self, id: KeyId) -> &str {
        &self.names[id.0 as usize]
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
    /// Column position + 1 by key id (0 = absent); key ids are small and dense, so this beats hashing.
    slots: Vec<u32>,
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
            self.slots[column.key.0 as usize] = 0;
        }
        self.columns.clear();
    }

    fn slot(&mut self, key: KeyId) -> &mut u32 {
        let idx = key.0 as usize;
        if idx >= self.slots.len() {
            self.slots.resize(idx + 1, 0);
        }
        &mut self.slots[idx]
    }

    pub(crate) fn add(&mut self, key: KeyId, value: PropRef<'_>) {
        let slot = *self.slot(key);
        let idx = if slot == 0 {
            self.columns.push(Column {
                key,
                kind: value.kind(),
                has_negative: false,
            });
            *self.slot(key) = index_u32(self.columns.len());
            self.columns.len() - 1
        } else {
            slot as usize - 1
        };
        let column = &mut self.columns[idx];
        column.kind = widen(column.kind, value.kind());
        column.has_negative |= matches!(value, PropRef::I64(v) if v < 0);
    }

    /// Final `(key, kind)` per column in column order; the index maps keys to positions in it.
    pub(crate) fn finish(&mut self, names: &KeyNames) -> Vec<(KeyId, PropKind)> {
        self.columns
            .sort_by(|a, b| names.column_order(a.key, b.key));
        for (pos, column) in self.columns.iter().enumerate() {
            self.slots[column.key.0 as usize] = index_u32(pos + 1);
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
        let slot = *self.slots.get(key.0 as usize)?;
        (slot != 0).then(|| slot as usize - 1)
    }
}

/// Matches mlt-core's inference for dynamically typed input: numbers mixing integers and floats widen
/// to `F64`, any other mix becomes a string column.
pub(crate) fn widen(a: PropKind, b: PropKind) -> PropKind {
    use PropKind::{F32, F64, I64, Str, U64};
    match (a, b) {
        _ if a == b => a,
        (I64 | U64, I64 | U64) => I64,
        (F32 | F64 | I64 | U64, F32 | F64 | I64 | U64) => F64,
        _ => Str,
    }
}

#[cfg(test)]
mod tests {
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
            columns.add(key, PropRef::I64(1));
        }
        let order: Vec<_> = columns
            .finish(&names)
            .iter()
            .map(|(k, _)| names.name(*k).to_owned())
            .collect();
        assert_eq!(order, ["zeta", "alpha", "a", "b"]);
    }

    #[test]
    fn kinds_widen_like_mlt_core() {
        let names = KeyInterner::new(["n", "f", "s", "u", "m"]).freeze();
        let mut columns = TileColumns::default();
        for (key, value) in [
            (0, PropRef::I64(-1)),
            (0, PropRef::I64(5)),
            (1, PropRef::F32(1.5)),
            (1, PropRef::F64(2.5)),
            (2, PropRef::I64(1)),
            (2, PropRef::Bool(true)),
            (3, PropRef::I64(7)),
            (4, PropRef::I64(3)),
            (4, PropRef::F64(1.5)),
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
            PropRef::I64(3).to_value(PropKind::F64),
            PropValue::F64(Some(3.0))
        );
        assert_eq!(
            PropRef::I64(1).to_value(PropKind::Str),
            PropValue::Str(Some("1".to_owned()))
        );
        assert_eq!(
            PropRef::F32(1.5).to_value(PropKind::F64),
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
