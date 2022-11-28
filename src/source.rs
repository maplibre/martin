use async_trait::async_trait;
use martin_tile_utils::DataFormat;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::fmt::{Debug, Display, Formatter};
use std::io;
use std::sync::{Arc, Mutex};
use tilejson::TileJSON;

#[derive(Debug, Copy, Clone)]
pub struct Xyz {
    pub z: i32,
    pub x: i32,
    pub y: i32,
}

impl Display for Xyz {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}/{}", self.z, self.x, self.y)
    }
}

pub type Tile = Vec<u8>;
pub type UrlQuery = HashMap<String, String>;

#[async_trait]
pub trait Source: Send + Debug {
    fn get_tilejson(&self) -> TileJSON;

    fn get_format(&self) -> DataFormat;

    fn clone_source(&self) -> Box<dyn Source>;

    fn is_valid_zoom(&self, zoom: i32) -> bool;

    async fn get_tile(&self, xyz: &Xyz, query: &Option<UrlQuery>) -> Result<Tile, io::Error>;
}

impl Clone for Box<dyn Source> {
    fn clone(&self) -> Self {
        self.clone_source()
    }
}

#[derive(Default, Clone)]
pub struct IdResolver {
    /// name -> unique name
    names: Arc<Mutex<HashMap<String, String>>>,
    /// reserved names
    reserved: HashSet<&'static str>,
}

impl IdResolver {
    pub fn new(reserved_keywords: &[&'static str]) -> Self {
        Self {
            names: Arc::new(Mutex::new(HashMap::new())),
            reserved: reserved_keywords.iter().copied().collect(),
        }
    }

    /// if name already exists in the self.names structure, but  try it with ".1", ".2", etc. until the value matches
    pub fn resolve(&self, mut name: String, unique_name: String) -> String {
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
            write!(&mut new_name, "{}.{}", name, index).unwrap();
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
        assert_eq!(r.resolve("a".to_string(), "a".to_string()), "a");
        assert_eq!(r.resolve("a".to_string(), "a".to_string()), "a");
        assert_eq!(r.resolve("a".to_string(), "b".to_string()), "a.1");
        assert_eq!(r.resolve("a".to_string(), "b".to_string()), "a.1");
        assert_eq!(r.resolve("b".to_string(), "a".to_string()), "b");
        assert_eq!(r.resolve("b".to_string(), "a".to_string()), "b");
        assert_eq!(r.resolve("a.1".to_string(), "a".to_string()), "a.1.1");
        assert_eq!(r.resolve("a.1".to_string(), "b".to_string()), "a.1");
    }
}
