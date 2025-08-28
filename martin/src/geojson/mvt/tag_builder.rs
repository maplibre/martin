use super::tile_value::TileValue;
use dup_indexer::{DupIndexer, PtrRead};
use std::hash::Hash;

/// A builder for key-value pairs, where the key is a `String` or `&str`, and the value is a
/// [`TileValue`] enum which can hold any of the MVT value types.
#[derive(Debug)]
pub struct TagsBuilder<K> {
    keys: DupIndexer<K>,
    values: DupIndexer<TileValue>,
}

/// This is safe because all values are either simple bit-readable values or strings,
/// both of which are safe for `PtrRead`.
unsafe impl PtrRead for TileValue {}

impl<K: Default + Eq + Hash + PtrRead> Default for TagsBuilder<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash + PtrRead> TagsBuilder<K> {
    pub fn new() -> Self {
        Self {
            keys: DupIndexer::new(),
            values: DupIndexer::new(),
        }
    }

    pub fn insert(&mut self, key: K, value: TileValue) -> (u32, u32) {
        (
            self.keys.insert(key) as u32,
            self.values.insert(value) as u32,
        )
    }

    pub fn into_tags(self) -> (Vec<K>, Vec<TileValue>) {
        (self.keys.into_vec(), self.values.into_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::TileValue::*;
    use super::*;

    fn s(s: &str) -> String {
        s.to_string()
    }

    #[test]
    fn test_add_value() {
        let mut lb = TagsBuilder::new();
        assert_eq!((0, 0), lb.insert(s("foo"), Str(s("bar"))));
        assert_eq!((0, 1), lb.insert(s("foo"), Str(s("baz"))));
        assert_eq!((0, 2), lb.insert(s("foo"), Int(42)));
        assert_eq!((1, 2), lb.insert(s("bar"), Int(42)));

        let (keys, values) = lb.into_tags();
        assert_eq!(vec![s("foo"), s("bar")], keys);
        assert_eq!(vec![Str(s("bar")), Str(s("baz")), Int(42)], values);
    }
}
