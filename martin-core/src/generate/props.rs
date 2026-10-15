//! Feature properties as they travel through the engine: interned keys and typed values.
//! Every property is dynamic: a tile's columns are whatever keys its features carry.

use lasso::{Key as _, RodeoResolver, Spur, ThreadedRodeo};
use mlt_core::PropValueRef;

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
        }
    }
}

/// The names of a [`KeyInterner`]'s keys once no more can appear.
#[derive(Debug)]
pub struct KeyNames {
    known: u32,
    names: RodeoResolver,
}

impl KeyNames {
    /// The name of `id`.
    #[must_use]
    pub fn name(&self, id: KeyId) -> &str {
        self.names.resolve(&id.spur())
    }

    /// Whether `id` was declared up front rather than found while reading.
    #[must_use]
    pub fn is_declared(&self, id: KeyId) -> bool {
        id.0 < self.known
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
