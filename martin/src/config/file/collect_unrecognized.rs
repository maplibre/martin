use std::collections::HashMap;
use std::collections::hash_map::IntoIter as HashMapIntoIter;

use serde::{Deserialize, Serialize};

/// Configuration keys that were present in a config section but not recognized by any known field.
///
/// Populated by `#[serde(flatten)]` capture on config structs and surfaced to the user as
/// warnings during finalization.
/// A newtype (rather than a bare `HashMap`) so it can carry its own `CollectUnrecognizedKeys`
/// behavior without overlapping the generic map impl.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct UnrecognizedValues(HashMap<String, serde_json::Value>);

impl UnrecognizedValues {
    /// Iterates over the unrecognized keys.
    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.0.keys()
    }

    /// Returns `true` if the given key was captured as unrecognized.
    pub fn contains_key(&self, key: impl AsRef<str>) -> bool {
        self.0.contains_key(key.as_ref())
    }

    /// Inserts an unrecognized key/value, returning the previous value if the key was present.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: serde_json::Value,
    ) -> Option<serde_json::Value> {
        self.0.insert(key.into(), value)
    }

    /// Removes and returns the value for the given key, if present.
    pub fn remove(&mut self, key: impl AsRef<str>) -> Option<serde_json::Value> {
        self.0.remove(key.as_ref())
    }
}

impl From<HashMap<String, serde_json::Value>> for UnrecognizedValues {
    fn from(values: HashMap<String, serde_json::Value>) -> Self {
        Self(values)
    }
}

impl<K: Into<String>> FromIterator<(K, serde_json::Value)> for UnrecognizedValues {
    fn from_iter<I: IntoIterator<Item = (K, serde_json::Value)>>(iter: I) -> Self {
        Self(iter.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }
}

impl IntoIterator for UnrecognizedValues {
    type Item = (String, serde_json::Value);
    type IntoIter = HashMapIntoIter<String, serde_json::Value>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
