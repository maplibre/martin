use std::collections::hash_map::IntoIter as HashMapIntoIter;
use std::collections::{BTreeMap, HashMap};
use std::num::{NonZeroI32, NonZeroU32, NonZeroU64, NonZeroUsize};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tilejson::Bounds;

#[cfg(all(feature = "webui", not(docsrs)))]
use crate::config::args::WebUiMode;
use crate::config::args::{BoundsCalcType, PreferredEncoding};
use crate::config::file::{
    CachePolicy, CacheSizeConfig, GlobalCacheConfig, OnInvalid, UnrecognizedKeys,
};

/// Configuration keys that were present in a config section but not recognized by any known field.
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

pub use martin_config_macros::CollectUnrecognizedKeys;

/// Collects unrecognized configuration keys as full dotted paths from the config root.
///
/// Derived with `#[derive(CollectUnrecognizedKeys)]`.
pub trait CollectUnrecognizedKeys {
    /// Collects unrecognized keys onto `path`, the dotted prefix `self`'s keys hang off.
    ///
    /// `path` ends with `.` (or is empty at the root), so a key is appended verbatim.
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys);

    /// Returns all unrecognized keys as full dotted paths from the config root.
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        let mut out = UnrecognizedKeys::new();
        self.collect_unrecognized("", &mut out);
        out
    }
}

impl CollectUnrecognizedKeys for UnrecognizedValues {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        for key in self.keys() {
            out.insert(format!("{path}{key}"));
        }
    }
}

impl<T: CollectUnrecognizedKeys> CollectUnrecognizedKeys for Option<T> {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        if let Some(value) = self {
            value.collect_unrecognized(path, out);
        }
    }
}

impl<T: CollectUnrecognizedKeys> CollectUnrecognizedKeys for Vec<T> {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        for (index, value) in self.iter().enumerate() {
            value.collect_unrecognized(&format!("{path}{index}."), out);
        }
    }
}

impl<K: AsRef<str>, V: CollectUnrecognizedKeys, S> CollectUnrecognizedKeys for HashMap<K, V, S> {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        for (key, value) in self {
            value.collect_unrecognized(&format!("{path}{}.", key.as_ref()), out);
        }
    }
}

impl<K: AsRef<str>, V: CollectUnrecognizedKeys> CollectUnrecognizedKeys for BTreeMap<K, V> {
    fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
        for (key, value) in self {
            value.collect_unrecognized(&format!("{path}{}.", key.as_ref()), out);
        }
    }
}

/// Implements `CollectUnrecognizedKeys` as a no-op for leaf types that carry no nested config.
macro_rules! impl_empty_collect_unrecognized {
    ($($t:ty),+ $(,)?) => {
        $(
            impl CollectUnrecognizedKeys for $t {
                fn collect_unrecognized(&self, _path: &str, _out: &mut UnrecognizedKeys) {}
            }
        )+
    };
}

impl_empty_collect_unrecognized!(
    bool,
    String,
    u8,
    u32,
    i32,
    u64,
    usize,
    f64,
    NonZeroU32,
    NonZeroU64,
    NonZeroI32,
    NonZeroUsize,
    PathBuf,
    Duration,
    serde_json::Value,
    Bounds,
    BoundsCalcType,
    OnInvalid,
    PreferredEncoding,
    CachePolicy,
    CacheSizeConfig,
    GlobalCacheConfig,
);

#[cfg(all(feature = "webui", not(docsrs)))]
impl_empty_collect_unrecognized!(WebUiMode);
