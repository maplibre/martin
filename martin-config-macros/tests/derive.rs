use config::file::{Bag, CollectUnrecognizedKeys};
use martin_config_macros::CollectUnrecognizedKeys;
use serde::Deserialize;

mod config {
    pub mod file {
        use std::collections::{BTreeMap, HashMap, HashSet};

        pub type UnrecognizedKeys = HashSet<String>;

        pub trait CollectUnrecognizedKeys {
            fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys);
            fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
                let mut out = UnrecognizedKeys::new();
                self.collect_unrecognized("", &mut out);
                out
            }
        }

        #[derive(Default, serde::Deserialize)]
        #[serde(transparent)]
        pub struct Bag(pub HashMap<String, i32>);

        impl CollectUnrecognizedKeys for Bag {
            fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
                out.extend(self.0.keys().map(|k| format!("{path}{k}")));
            }
        }

        impl CollectUnrecognizedKeys for bool {
            fn collect_unrecognized(&self, _: &str, _: &mut UnrecognizedKeys) {}
        }

        impl<T: CollectUnrecognizedKeys> CollectUnrecognizedKeys for Vec<T> {
            fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
                let base = path.strip_suffix('.').unwrap_or(path);
                for (i, v) in self.iter().enumerate() {
                    v.collect_unrecognized(&format!("{base}[{i}]."), out);
                }
            }
        }

        impl<V: CollectUnrecognizedKeys> CollectUnrecognizedKeys for BTreeMap<String, V> {
            fn collect_unrecognized(&self, path: &str, out: &mut UnrecognizedKeys) {
                for (k, v) in self {
                    v.collect_unrecognized(&format!("{path}{k}."), out);
                }
            }
        }
    }
}

fn bag(keys: &[&str]) -> Bag {
    Bag(keys.iter().map(|k| ((*k).to_string(), 0)).collect())
}

fn keys(value: &impl CollectUnrecognizedKeys) -> Vec<String> {
    let mut keys: Vec<String> = value.get_unrecognized_keys().into_iter().collect();
    keys.sort_unstable();
    keys
}

#[derive(Deserialize, CollectUnrecognizedKeys)]
struct Inner {
    known: bool,
    #[serde(flatten)]
    unrecognized: Bag,
}

fn inner(unrecognized: &[&str]) -> Inner {
    Inner {
        known: true,
        unrecognized: bag(unrecognized),
    }
}

#[test]
fn flatten_adds_no_segment_and_leaves_are_ignored() {
    assert_eq!(keys(&inner(&["a", "b"])), ["a", "b"]);
}

#[derive(CollectUnrecognizedKeys)]
struct Nested {
    child: Inner,
}

#[test]
fn named_field_adds_its_name() {
    assert_eq!(
        keys(&Nested {
            child: inner(&["typo"])
        }),
        ["child.typo"]
    );
}

#[derive(CollectUnrecognizedKeys)]
struct WithVec {
    items: Vec<Inner>,
}

#[test]
fn vec_uses_bracketed_indices() {
    let value = WithVec {
        items: vec![inner(&["a"]), inner(&["b"])],
    };
    assert_eq!(keys(&value), ["items[0].a", "items[1].b"]);
}

#[derive(CollectUnrecognizedKeys)]
struct WithMap {
    entries: std::collections::BTreeMap<String, Inner>,
}

#[test]
fn map_uses_key_segments() {
    let entries = std::collections::BTreeMap::from([("first".to_string(), inner(&["a"]))]);
    assert_eq!(keys(&WithMap { entries }), ["entries.first.a"]);
}

#[derive(Deserialize, CollectUnrecognizedKeys)]
struct WithSkip {
    #[serde(skip)]
    #[allow(dead_code)]
    ignored: Bag,
    #[serde(flatten)]
    unrecognized: Bag,
}

#[test]
fn skipped_fields_are_not_collected() {
    let value = WithSkip {
        ignored: bag(&["skipped"]),
        unrecognized: bag(&["kept"]),
    };
    assert_eq!(keys(&value), ["kept"]);
}

#[derive(Deserialize, CollectUnrecognizedKeys)]
struct WithRename {
    #[serde(rename = "renamed")]
    child: Inner,
}

#[test]
fn rename_sets_the_segment() {
    assert_eq!(
        keys(&WithRename {
            child: inner(&["typo"])
        }),
        ["renamed.typo"]
    );
}

#[derive(CollectUnrecognizedKeys)]
enum Variants {
    Empty,
    Wrapped(Inner),
}

#[test]
fn enum_dispatches_to_active_variant() {
    assert!(keys(&Variants::Empty).is_empty());
    assert_eq!(keys(&Variants::Wrapped(inner(&["typo"]))), ["typo"]);
}
