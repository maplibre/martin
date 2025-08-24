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
    /// Creates a new `IdResolver` with the given reserved keywords.
    ///
    /// Assumes that reserved keywords never end in a "dot number" (e.g., "catalog.1")
    #[must_use]
    pub fn new(reserved_keywords: &[&'static str]) -> Self {
        Self {
            names: Arc::new(Mutex::new(HashMap::new())),
            reserved: reserved_keywords.iter().copied().collect(),
        }
    }

    /// Makes sure that every source has a unique, non-reserved name
    ///
    /// Replace non-alphanumeric characters or dashes/dots/underscores with dashes.
    /// If an unique source name already exists in the self.names structure ".1", ".2", etc. is appended.
    /// For every name which is changed, a warning is logged.
    ///
    /// ```
    /// let reserved = &["catalog"];
    /// let r = martin::IdResolver::new(reserved);
    ///
    /// // catalog is a reserved name => needs renaming
    /// assert_eq!(r.resolve("catalog", "catalog1".to_string()), "catalog.1");
    /// // same unique_name => same index
    /// assert_eq!(r.resolve("catalog", "catalog1".to_string()), "catalog.1");
    /// // different unique_name => different index
    /// assert_eq!(r.resolve("catalog", "catalog2".to_string()), "catalog.2");
    ///
    /// // disallowed characters are replaced with underscores
    /// assert_eq!(r.resolve("name with disallowed chäractérs 😃", "".to_string()), "name-with-disallowed-ch-ract-rs--");
    /// assert_eq!(r.resolve("name-with_allowed.chars", "".to_string()), "name-with_allowed.chars");
    ///
    /// // not a reserved name => no renaming
    /// assert_eq!(r.resolve("different_name", "different_name1".to_string()), "different_name");
    /// // same unique_name => same index
    /// assert_eq!(r.resolve("different_name", "different_name1".to_string()), "different_name");
    /// // different unique_name => different index
    /// assert_eq!(r.resolve("different_name", "different_name2".to_string()), "different_name.1");
    /// ```
    #[must_use]
    pub fn resolve(&self, name: &str, unique_name: String) -> String {
        let info = if name == unique_name {
            None
        } else {
            Some(unique_name.clone())
        };
        let stanitised_name = Self::sanitise(name);

        let is_reserved_name = self.reserved.contains(stanitised_name.as_str());
        let mut names = self.names.lock().expect("IdResolver panicked");
        // simple case if names need not be renamed
        if !is_reserved_name {
            match names.entry(stanitised_name.clone()) {
                Entry::Vacant(e) => {
                    if stanitised_name != name {
                        warn!(
                            "Source `{name}`{info} was renamed to `{stanitised_name}` because ID may only contain alpha-numeric characters or `._-`",
                            info = info.map_or(String::new(), |v| format!(" ({v})"))
                        );
                    }
                    e.insert(unique_name);
                    return stanitised_name;
                }
                Entry::Occupied(e) => {
                    if e.get() == &unique_name {
                        return stanitised_name;
                    }
                }
            }
        }

        // need to rename => try ".1", ".2", etc. until the value matches
        // assumes that reserved keywords never end in a "dot number" => no special case for this
        let mut index: i32 = 1;
        let mut new_name = String::new();
        loop {
            new_name.clear();
            write!(&mut new_name, "{stanitised_name}.{index}").unwrap();
            index = index.checked_add(1).unwrap();
            match names.entry(new_name.clone()) {
                // found new name
                Entry::Vacant(e) => {
                    if is_reserved_name {
                        warn!(
                            "`{name}` is a reserved keyword, and may not be used as a source. The source was renamed to `{new_name}`{info}",
                            info = info.map_or(String::new(), |v| format!(" ({v})"))
                        );
                    } else {
                        warn!(
                            "Source `{name}`{info} was renamed to `{new_name}` because ID must be unique, and only contain alpha-numeric characters or `._-`",
                            info = info.map_or(String::new(), |v| format!(" ({v})"))
                        );
                    }
                    e.insert(unique_name);
                    return new_name;
                }
                Entry::Occupied(e) => {
                    if e.get() == &unique_name {
                        // found existing name
                        return new_name;
                    }
                }
            }
        }
    }
    /// replace prohibited characters, except underscores, dashes, and dots with dashes.
    fn sanitise(name: &str) -> String {
        name.replace(
            |c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '.' && c != '-',
            "-",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_resolve() {
        let reserved = ["reserved"];
        let r = IdResolver::new(&reserved);

        assert_eq!(r.resolve("a", "a".to_string()), "a");
        assert_eq!(r.resolve("a", "a".to_string()), "a");
        assert_eq!(r.resolve("a", "b".to_string()), "a.1");
        assert_eq!(r.resolve("a", "b".to_string()), "a.1");
        assert_eq!(r.resolve("b", "a".to_string()), "b");
        assert_eq!(r.resolve("b", "a".to_string()), "b");
        // reserved
        assert_eq!(r.resolve("reserved", "a".to_string()), "reserved.1");
        assert_eq!(r.resolve("reserved", "a".to_string()), "reserved.1");
        assert_eq!(r.resolve("reserved", "b".to_string()), "reserved.2");
        // special characters
        assert_eq!(r.resolve("a.1", "a".to_string()), "a.1.1");
        assert_eq!(r.resolve("a.1", "b".to_string()), "a.1");
        assert_eq!(r.resolve("a b", "a b".to_string()), "a-b");
        assert_eq!(r.resolve("a b", "ab2".to_string()), "a-b.1");
    }
}
