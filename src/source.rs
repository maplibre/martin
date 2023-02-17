use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::{Debug, Display, Formatter, Write};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use martin_tile_utils::DataFormat;
use tilejson::TileJSON;

use crate::utils::Result;

#[derive(Debug, Copy, Clone)]
pub struct Xyz {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

impl Display for Xyz {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if f.alternate() {
            write!(f, "{}/{}/{}", self.z, self.x, self.y)
        } else {
            write!(f, "{},{},{}", self.z, self.x, self.y)
        }
    }
}

pub type Tile = Vec<u8>;
pub type UrlQuery = HashMap<String, String>;
pub type Sources = HashMap<String, Box<dyn Source>>;

#[async_trait]
pub trait Source: Send + Debug {
    fn get_tilejson(&self) -> TileJSON;

    fn get_format(&self) -> DataFormat;

    fn clone_source(&self) -> Box<dyn Source>;

    fn is_valid_zoom(&self, zoom: u8) -> bool;

    fn support_url_query(&self) -> bool;

    async fn get_tile(&self, xyz: &Xyz, query: &Option<UrlQuery>) -> Result<Tile>;
}

impl Clone for Box<dyn Source> {
    fn clone(&self) -> Self {
        self.clone_source()
    }
}

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

    #[test]
    fn xyz_format() {
        let xyz = Xyz { z: 1, x: 2, y: 3 };
        assert_eq!(format!("{xyz}"), "1,2,3");
        assert_eq!(format!("{xyz:#}"), "1/2/3");
    }
}
