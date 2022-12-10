use log::{error, info, warn};
use std::collections::HashMap;

pub type InfoMap<T> = HashMap<String, T>;

pub fn normalize_key<'a, T>(
    map: &'a InfoMap<T>,
    key: &str,
    info: &str,
    id: &str,
) -> Option<String> {
    find_info_kv(map, key, info, id).map(|(k, _)| k.to_string())
}

pub fn find_info<'a, T>(map: &'a InfoMap<T>, key: &'a str, info: &str, id: &str) -> Option<&'a T> {
    find_info_kv(map, key, info, id).map(|(_, v)| v)
}

pub fn find_info_kv<'a, T>(
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
            info!("For source {id}, {info} '{key}' was not found, using '{result}' instead.");
            Some((result.as_str(), map.get(result)?))
        } else {
            warn!("Unable to configure source {id} because {info} '{key}' was not found.  Possible values are: {}",
                map.keys().map(|k| k.as_str()).collect::<Vec<_>>().join(", "));
            None
        }
    } else {
        error!("Unable to configure source {id} because {info} '{key}' has no exact match and more than one potential matches: {}", multiple.join(", "));
        None
    }
}
