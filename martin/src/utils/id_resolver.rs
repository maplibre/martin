use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use log::warn;

#[derive(Debug, Default, Clone)]
pub struct IdResolver {
    /// name -> unique name
    names: Arc<Mutex<HashMap<String, String>>>,
    /// reserved names
    reserved: HashSet<&'static str>,
}

impl IdResolver {
    #[must_use]
    pub fn new(reserved_keywords: &[&'static str]) -> Self {
        Self {
            names: Arc::new(Mutex::new(HashMap::new())),
            reserved: reserved_keywords.iter().copied().collect(),
        }
    }

    /// If source name already exists in the self.names structure,
    /// try appending it with ".1", ".2", etc. until the name is unique.
    /// Only alphanumeric characters plus dashes/dots/underscores are allowed.
    #[must_use]
    pub fn resolve(&self, name: &str, unique_name: String) -> String {
        let info = if name == unique_name {
            None
        } else {
            Some(unique_name.clone())
        };
        let new_name = self.resolve_int(name, unique_name);
        if name != new_name {
            warn!(
                "Source `{name}`{info} was renamed to `{new_name}`. Source IDs must be unique, cannot be reserved, and must contain alpha-numeric characters or `._-`",
                info = info.map_or(String::new(), |v| format!(" ({v})"))
            );
        }
        new_name
    }

    #[must_use]
    fn resolve_int(&self, name: &str, unique_name: String) -> String {
        // Ensure name has no prohibited characters like spaces, commas, slashes, or non-unicode etc.
        // Underscores, dashes, and dots are OK. All other characters will be replaced with dashes.
        let mut name = name.replace(
            |c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.' && c != '-',
            "-",
        );

        let mut names = self.names.lock().expect("IdResolver panicked");
        if !self.reserved.contains(name.as_str()) {
            match names.entry(name) {
                Entry::Vacant(e) => {
                    let id = e.key().clone();
                    e.insert(unique_name);
                    return id;
                }
                // Rust v1.78 - possibly due to bug fixed in https://github.com/rust-lang/rust-clippy/pull/12756
                #[allow(unknown_lints, clippy::assigning_clones)]
                Entry::Occupied(e) => {
                    name = e.key().clone();
                    if e.get() == &unique_name {
                        return name;
                    }
                }
            }
        }
        // name already exists, try it with ".1", ".2", etc. until the value matches
        // assume that reserved keywords never end in a "dot number", so don't check
        let mut index: i32 = 1;
        let mut new_name = String::new();
        loop {
            new_name.clear();
            write!(&mut new_name, "{name}.{index}").unwrap();
            index = index.checked_add(1).unwrap();
            match names.entry(new_name.clone()) {
                Entry::Vacant(e) => {
                    e.insert(unique_name);
                    return new_name;
                }
                Entry::Occupied(e) => {
                    if e.get() == &unique_name {
                        return new_name;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_resolve() {
        let r = IdResolver::default();
        assert_eq!(r.resolve("a", "a".to_string()), "a");
        assert_eq!(r.resolve("a", "a".to_string()), "a");
        assert_eq!(r.resolve("a", "b".to_string()), "a.1");
        assert_eq!(r.resolve("a", "b".to_string()), "a.1");
        assert_eq!(r.resolve("b", "a".to_string()), "b");
        assert_eq!(r.resolve("b", "a".to_string()), "b");
        assert_eq!(r.resolve("a.1", "a".to_string()), "a.1.1");
        assert_eq!(r.resolve("a.1", "b".to_string()), "a.1");

        assert_eq!(r.resolve("a b", "a b".to_string()), "a-b");
        assert_eq!(r.resolve("a b", "ab2".to_string()), "a-b.1");
    }
}
