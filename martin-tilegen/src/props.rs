//! Feature properties as they travel through the engine: interned keys and borrowed typed values.
//! Every property is dynamic: a tile's columns are whatever keys its features carry.

use std::cmp::Ordering;

use lasso::{Key as _, RodeoResolver, Spur, ThreadedRodeo};
use mlt_core::{PropKind, PropValue, PropValueRef};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(pub(crate) u32);

impl KeyId {
    fn spur(self) -> Spur {
        Spur::try_from_usize(self.0 as usize).expect("key ids come from a Spur")
    }
}

fn kind(value: PropValueRef<'_>) -> PropKind {
    match value {
        PropValueRef::Bool(_) => PropKind::Bool,
        PropValueRef::I8(_)
        | PropValueRef::U8(_)
        | PropValueRef::I32(_)
        | PropValueRef::U32(_)
        | PropValueRef::I64(_) => PropKind::I64,
        PropValueRef::U64(_) => PropKind::U64,
        PropValueRef::F32(_) => PropKind::F32,
        PropValueRef::F64(_) => PropKind::F64,
        PropValueRef::Str(_) => PropKind::Str,
    }
}

fn as_i64(value: PropValueRef<'_>) -> Option<i64> {
    match value {
        PropValueRef::I8(v) => Some(v.into()),
        PropValueRef::U8(v) => Some(v.into()),
        PropValueRef::I32(v) => Some(v.into()),
        PropValueRef::U32(v) => Some(v.into()),
        PropValueRef::I64(v) => Some(v),
        PropValueRef::Bool(_)
        | PropValueRef::U64(_)
        | PropValueRef::F32(_)
        | PropValueRef::F64(_)
        | PropValueRef::Str(_) => None,
    }
}

/// The value in the column type a tile settled on (see [`TileColumns`]).
#[expect(
    clippy::cast_precision_loss,
    reason = "as mlt-core converts integers in float columns"
)]
pub(crate) fn to_value(value: PropValueRef<'_>, kind: PropKind) -> PropValue {
    match (kind, value, as_i64(value)) {
        (PropKind::Bool, PropValueRef::Bool(v), _) => PropValue::Bool(Some(v)),
        (PropKind::I64, _, Some(v)) => PropValue::I64(Some(v)),
        (PropKind::I64, PropValueRef::U64(v), _) => PropValue::I64(i64::try_from(v).ok()),
        (PropKind::U64, _, Some(v)) => PropValue::U64(u64::try_from(v).ok()),
        (PropKind::U64, PropValueRef::U64(v), _) => PropValue::U64(Some(v)),
        (PropKind::F32, PropValueRef::F32(v), _) => PropValue::F32(Some(v)),
        (PropKind::F64, PropValueRef::F32(v), _) => PropValue::F64(Some(f64::from(v))),
        (PropKind::F64, PropValueRef::F64(v), _) => PropValue::F64(Some(v)),
        (PropKind::F64, _, Some(v)) => PropValue::F64(Some(v as f64)),
        (PropKind::F64, PropValueRef::U64(v), _) => PropValue::F64(Some(v as f64)),
        _ => PropValue::Str(Some(to_text(value))),
    }
}

fn to_text(value: PropValueRef<'_>) -> String {
    match value {
        PropValueRef::Str(v) => v.to_owned(),
        PropValueRef::Bool(v) => v.to_string(),
        PropValueRef::I8(v) => v.to_string(),
        PropValueRef::U8(v) => v.to_string(),
        PropValueRef::I32(v) => v.to_string(),
        PropValueRef::U32(v) => v.to_string(),
        PropValueRef::I64(v) => v.to_string(),
        PropValueRef::U64(v) => v.to_string(),
        PropValueRef::F32(v) => v.to_string(),
        PropValueRef::F64(v) => v.to_string(),
    }
}

fn index_u32(len: usize) -> u32 {
    u32::try_from(len).expect("fewer than 2^32 property keys or columns")
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

    pub(crate) fn add(&mut self, key: KeyId, value: PropValueRef<'_>) {
        let slot = *self.slot(key);
        let idx = if slot == 0 {
            self.columns.push(Column {
                key,
                kind: kind(value),
                has_negative: false,
            });
            *self.slot(key) = index_u32(self.columns.len());
            self.columns.len() - 1
        } else {
            slot as usize - 1
        };
        let column = &mut self.columns[idx];
        column.kind = widen(column.kind, kind(value));
        column.has_negative |= as_i64(value).is_some_and(|v| v < 0);
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
    fn kinds_widen_like_mlt_core() {
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
            to_value(PropValueRef::I64(3), PropKind::F64),
            PropValue::F64(Some(3.0))
        );
        assert_eq!(
            to_value(PropValueRef::I64(1), PropKind::Str),
            PropValue::Str(Some("1".to_owned()))
        );
        assert_eq!(
            to_value(PropValueRef::F32(1.5), PropKind::F64),
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
