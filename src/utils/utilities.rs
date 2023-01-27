use std::cmp::Ordering::Equal;
use std::collections::{BTreeMap, HashMap};

use itertools::Itertools;
use log::{error, info, warn};
use serde::{Deserialize, Serialize, Serializer};

pub type InfoMap<T> = HashMap<String, T>;

#[must_use]
pub fn normalize_key<T>(map: &InfoMap<T>, key: &str, info: &str, id: &str) -> Option<String> {
    find_info_kv(map, key, info, id).map(|(k, _)| k.to_string())
}

#[must_use]
pub fn find_info<'a, T>(map: &'a InfoMap<T>, key: &'a str, info: &str, id: &str) -> Option<&'a T> {
    find_info_kv(map, key, info, id).map(|(_, v)| v)
}

#[must_use]
fn find_info_kv<'a, T>(
    map: &'a InfoMap<T>,
    key: &'a str,
    info: &str,
    id: &str,
) -> Option<(&'a str, &'a T)> {
    if let Some(v) = map.get(key) {
        return Some((key, v));
    }

    let mut result = None;
    let mut multiple = Vec::new();
    for k in map.keys() {
        if k.to_lowercase() == key.to_lowercase() {
            match result {
                None => result = Some(k),
                Some(result) => {
                    if multiple.is_empty() {
                        multiple.push(result.to_string());
                    }
                    multiple.push(k.to_string());
                }
            }
        }
    }

    if multiple.is_empty() {
        if let Some(result) = result {
            info!("For source {id}, {info} '{key}' was not found, but found '{result}' instead.");
            Some((result.as_str(), map.get(result)?))
        } else {
            warn!("Unable to configure source {id} because {info} '{key}' was not found.  Possible values are: {}",
                map.keys().map(String::as_str).collect::<Vec<_>>().join(", "));
            None
        }
    } else {
        error!("Unable to configure source {id} because {info} '{key}' has no exact match and more than one potential matches: {}",
            multiple.join(", "));
        None
    }
}

#[must_use]
pub fn is_valid_zoom(zoom: u8, minzoom: Option<u8>, maxzoom: Option<u8>) -> bool {
    minzoom.map_or(true, |minzoom| zoom >= minzoom)
        && maxzoom.map_or(true, |maxzoom| zoom <= maxzoom)
}

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrObject<T> {
    Bool(bool),
    Object(T),
}

/// Sort an optional hashmap by key, case-insensitive first, then case-sensitive
pub fn sorted_opt_map<S: Serializer, T: Serialize>(
    value: &Option<HashMap<String, T>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    value
        .as_ref()
        .map(|v| {
            v.iter()
                .sorted_by(|a, b| {
                    let lower = a.0.to_lowercase().cmp(&b.0.to_lowercase());
                    match lower {
                        Equal => a.0.cmp(b.0),
                        other => other,
                    }
                })
                .collect::<BTreeMap<_, _>>()
        })
        .serialize(serializer)
}
