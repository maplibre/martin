//! Feature properties as they travel through the engine: interned keys and typed values.
//! Every property is dynamic: a tile's columns are whatever keys its features carry.

use std::borrow::Cow;

pub use bytestring::ByteString;
use lasso::{Capacity, Key as _, Rodeo, RodeoResolver, Spur, ThreadedRodeo};
use mlt_core::{PropKind, PropValueRef};

/// A source's text value, a slice of a buffer shared with the rest of its batch.
pub type SourceStr = ByteString;

/// An interned property key of one layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId(u32);

impl KeyId {
    /// The id of the `n`th key declared up front: a source can use column positions without interning.
    #[must_use]
    pub const fn new(n: u32) -> Self {
        Self(n)
    }

    /// Position of the key among all keys of its layer.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }

    /// [`get`](Self::get) as an index.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }

    fn from_spur(spur: Spur) -> Self {
        Self(index_u32(spur.into_usize()))
    }

    fn spur(self) -> Spur {
        Spur::try_from_usize(self.index()).expect("key ids come from a Spur")
    }
}

/// One property value of a feature.
#[derive(Clone, Debug, PartialEq)]
#[expect(missing_docs)]
pub enum Prop<S = String> {
    Bool(bool),
    I64(i64),
    F32(f32),
    F64(f64),
    Str(S),
    /// A `jsonb` document kept whole, as its JSON text.
    ///
    /// The text is parsed once the column's shape is known, so it is not typed before the sort.
    /// Until then a reader that wants a scalar sees the text, as [`Str`](Self::Str) does.
    #[cfg(feature = "unstable-mlt-v2")]
    Nested(S),
}

impl<S: AsRef<str>> Prop<S> {
    /// Borrows the value as an MLT property value.
    #[must_use]
    pub fn as_ref(&self) -> PropValueRef<'_> {
        match self {
            Self::Bool(v) => PropValueRef::Bool(*v),
            Self::I64(v) => PropValueRef::I64(*v),
            Self::F32(v) => PropValueRef::F32(*v),
            Self::F64(v) => PropValueRef::F64(*v),
            Self::Str(v) => PropValueRef::Str(v.as_ref()),
            #[cfg(feature = "unstable-mlt-v2")]
            Self::Nested(v) => PropValueRef::Str(v.as_ref()),
        }
    }

    /// Borrows the value, keeping its text as a `Cow` that expressions can replace with an owned result.
    #[must_use]
    pub fn as_cow(&self) -> Prop<Cow<'_, str>> {
        match self {
            Self::Bool(v) => Prop::Bool(*v),
            Self::I64(v) => Prop::I64(*v),
            Self::F32(v) => Prop::F32(*v),
            Self::F64(v) => Prop::F64(*v),
            Self::Str(v) => Prop::Str(Cow::Borrowed(v.as_ref())),
            #[cfg(feature = "unstable-mlt-v2")]
            Self::Nested(v) => Prop::Nested(Cow::Borrowed(v.as_ref())),
        }
    }
}

fn index_u32(len: usize) -> u32 {
    u32::try_from(len).expect("fewer than 2^32 property keys")
}

/// Property keys of one layer, shared by all workers. Keys the source declares up front get the first ids
/// in declaration order, which is their column order in every tile; keys found later sort by name.
#[derive(Debug)]
pub struct KeyInterner {
    known: u32,
    keys: ThreadedRodeo,
}

impl KeyInterner {
    /// An interner whose first ids are `known`, in order.
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

    /// The id of `name`, interning it if it is new.
    pub fn intern(&self, name: &str) -> KeyId {
        KeyId::from_spur(self.keys.get_or_intern(name))
    }

    /// The id of `name`, if it was interned.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<KeyId> {
        self.keys.get(name).map(KeyId::from_spur)
    }

    /// The name of `id`, if it was interned.
    #[must_use]
    pub fn name(&self, id: KeyId) -> Option<String> {
        Spur::try_from_usize(id.index())
            .and_then(|spur| self.keys.try_resolve(&spur))
            .map(str::to_owned)
    }

    /// Call once no more keys can appear, i.e. after rendering.
    #[must_use]
    pub fn freeze(self) -> KeyNames {
        KeyNames {
            known: self.known,
            names: self.keys.into_resolver(),
            kinds: Vec::new(),
        }
    }

    /// [`freeze`](Self::freeze), copying the names.
    #[must_use]
    pub fn names(&self) -> KeyNames {
        let mut names = Rodeo::with_capacity(Capacity::for_strings(self.keys.len()));
        for id in 0..index_u32(self.keys.len()) {
            names.get_or_intern(self.keys.resolve(&KeyId::new(id).spur()));
        }
        KeyNames {
            known: self.known,
            names: names.into_resolver(),
            kinds: Vec::new(),
        }
    }
}

/// The names of a [`KeyInterner`]'s keys once no more can appear, and the kinds their values took.
#[derive(Debug)]
pub struct KeyNames {
    known: u32,
    names: RodeoResolver,
    /// By key; empty when no render tallied them.
    kinds: Vec<KeyKind>,
}

impl KeyNames {
    /// These names with the kinds every value of each key took, so each key gets one column kind in
    /// every tile.
    #[must_use]
    pub(crate) fn with_kinds(mut self, kinds: &[KeyKind]) -> Self {
        kinds.clone_into(&mut self.kinds);
        self
    }

    /// The kind every value of `id` took, if any were tallied.
    pub(crate) fn kind(&self, id: KeyId) -> Option<KeyKind> {
        self.kinds.get(id.index()).copied().filter(|kind| kind.kind.is_some())
    }

    /// The name of `id`.
    #[must_use]
    pub fn name(&self, id: KeyId) -> &str {
        self.names.resolve(&id.spur())
    }

    /// The names declared up front, in declaration order.
    pub fn declared(&self) -> impl Iterator<Item = &str> {
        (0..self.known).map(|id| self.name(KeyId::new(id)))
    }

    /// Every key and its name.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (KeyId, &str)> {
        self.names.iter().map(|(spur, name)| (KeyId::from_spur(spur), name))
    }

    /// Whether `id` was declared up front rather than found while reading.
    #[must_use]
    pub fn is_declared(&self, id: KeyId) -> bool {
        id.0 < self.known
    }
}

/// The kinds every value of one key took, merged as [`PropKind::merge`] merges a column's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct KeyKind {
    /// `None` until a value is tallied.
    kind: Option<PropKind>,
    /// Whether an integer value was negative; without one, an `I64` column is stored as `U64`.
    pub(crate) has_negative: bool,
    /// Whether values of kinds no single column holds made the kind `Str`.
    pub(crate) mixed: bool,
}

impl KeyKind {
    /// The merged kind; a tallied key always has one.
    pub(crate) fn merged(self) -> PropKind {
        self.kind.unwrap_or(PropKind::Str)
    }

    fn add_kind(&mut self, kind: PropKind, negative: bool) {
        let merged = self.kind.map_or(kind, |known| known.merge(kind));
        let was_other = self.kind.is_some_and(|known| known != PropKind::Str) || kind != PropKind::Str;
        self.mixed |= merged == PropKind::Str && self.kind.is_some() && was_other;
        self.kind = Some(merged);
        self.has_negative |= negative;
    }

    /// Tallies `value` as the sort record stores it: narrower integers as `I64`.
    fn add(&mut self, value: PropValueRef<'_>) {
        let (kind, negative) = match value {
            PropValueRef::I8(v) => (PropKind::I64, v < 0),
            PropValueRef::I32(v) => (PropKind::I64, v < 0),
            PropValueRef::I64(v) => (PropKind::I64, v < 0),
            PropValueRef::U8(_) | PropValueRef::U32(_) => (PropKind::I64, false),
            PropValueRef::Bool(_) => (PropKind::Bool, false),
            PropValueRef::U64(_) => (PropKind::U64, false),
            PropValueRef::F32(_) => (PropKind::F32, false),
            PropValueRef::F64(_) => (PropKind::F64, false),
            PropValueRef::Str(_) => (PropKind::Str, false),
        };
        self.add_kind(kind, negative);
    }

    fn merge(&mut self, other: Self) {
        if let Some(kind) = other.kind {
            self.add_kind(kind, other.has_negative);
        }
        self.mixed |= other.mixed;
    }
}

/// The [`KeyKind`] of every key of every layer, tallied while rendering.
#[derive(Debug, Default)]
pub(crate) struct LayerKinds(Vec<Vec<KeyKind>>);

impl LayerKinds {
    /// Tallies `value` of `key` in layer `layer`.
    pub(crate) fn add(&mut self, layer: usize, key: KeyId, value: PropValueRef<'_>) {
        if layer >= self.0.len() {
            self.0.resize_with(layer + 1, Vec::new);
        }
        let keys = &mut self.0[layer];
        if key.index() >= keys.len() {
            keys.resize(key.index() + 1, KeyKind::default());
        }
        keys[key.index()].add(value);
    }

    /// Adds another tally to this one.
    pub(crate) fn merge(&mut self, other: Self) {
        if self.0.len() < other.0.len() {
            self.0.resize_with(other.0.len(), Vec::new);
        }
        for (keys, others) in self.0.iter_mut().zip(other.0) {
            if keys.len() < others.len() {
                keys.resize(others.len(), KeyKind::default());
            }
            for (kind, other) in keys.iter_mut().zip(others) {
                kind.merge(other);
            }
        }
    }

    /// The kinds of layer `layer`'s keys, by key.
    pub(crate) fn layer(&self, layer: usize) -> &[KeyKind] {
        self.0.get(layer).map_or(&[], Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "unstable-mlt-v2")]
    #[test]
    fn nested_reads_as_its_text() {
        let prop = Prop::Nested("{\"a\":[]}".to_owned());
        assert_eq!(prop.as_ref(), PropValueRef::Str("{\"a\":[]}"));
        assert_eq!(prop.as_cow(), Prop::Nested(Cow::Borrowed("{\"a\":[]}")));
        assert_ne!(prop.clone(), Prop::Str("{\"a\":[]}".to_owned()));
    }

    #[test]
    fn interning_is_thread_safe() {
        let interner = KeyInterner::new::<&str>([]);

        let ids: Vec<Vec<KeyId>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..4)
                .map(|_| scope.spawn(|| (0..100).map(|i| interner.intern(&format!("k{i}"))).collect()))
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
