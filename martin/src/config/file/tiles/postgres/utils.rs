use std::collections::BTreeMap;

use itertools::Itertools;
use log::error;
use tilejson::TileJSON;

#[must_use]
pub fn normalize_key<T>(
    map: &BTreeMap<String, T>,
    key: &str,
    info: &str,
    id: &str,
) -> Option<String> {
    find_info_kv(map, key, info, id)
        .map(|(k, _)| k.to_string())
        .ok()
}

pub fn find_info<'a, T>(
    map: &'a BTreeMap<String, T>,
    key: &'a str,
    info: &str,
    id: &str,
) -> Result<&'a T, String> {
    find_info_kv(map, key, info, id).map(|(_, v)| v)
}

fn find_info_kv<'a, T>(
    map: &'a BTreeMap<String, T>,
    key: &'a str,
    info: &str,
    id: &str,
) -> Result<(&'a str, &'a T), String> {
    if let Some(v) = map.get(key) {
        return Ok((key, v));
    }

    match find_kv_ignore_case(map, key) {
        Ok(None) => Err(format!(
            "Unable to configure source {id} because {info} '{key}' was not found.  Possible values are: {}",
            map.keys().map(String::as_str).join(", ")
        )),
        Ok(Some(result)) => map.get(result).map(|v| (result.as_str(), v)).ok_or(format!(
            "For source {id}, {info} '{key}' was not found, but found '{result}' instead."
        )),
        Err(multiple) => Err(format!(
            "Unable to configure source {id} because {info} '{key}' has no exact match and more than one potential matches: {}",
            multiple.join(", ")
        )),
    }
}

/// Find a key in a map, ignoring case.
///
/// If there is no exact match, but there is a case-insensitive match, return that as `Ok(Some(value))`.
/// If there is no exact match and there are multiple case-insensitive matches, return an error with a vector of the possible matches.
/// If there is no match, return `Ok(None)`.
pub fn find_kv_ignore_case<'a, T>(
    map: &'a BTreeMap<String, T>,
    key: &str,
) -> Result<Option<&'a String>, Vec<String>> {
    let key = key.to_lowercase();
    let mut result = None;
    let mut multiple = Vec::new();
    for k in map.keys() {
        if k.to_lowercase() == key {
            match result {
                None => result = Some(k),
                Some(result) => {
                    if multiple.is_empty() {
                        multiple.push(result.clone());
                    }
                    multiple.push(k.clone());
                }
            }
        }
    }
    if multiple.is_empty() {
        Ok(result)
    } else {
        Err(multiple)
    }
}

#[must_use]
pub fn patch_json(target: TileJSON, patch: Option<&serde_json::Value>) -> TileJSON {
    let Some(tj) = patch else {
        // Nothing to merge in, keep the original
        return target;
    };
    // Not the most efficient, but this is only executed once per source:
    // * Convert the TileJSON struct to a serde_json::Value
    // * Merge the self.tilejson into the value
    // * Convert the merged value back to a TileJSON struct
    // * In case of errors, return the original tilejson
    let mut tilejson2 = match serde_json::to_value(target.clone()) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to serialize tilejson, unable to merge function comment: {e}");
            return target;
        }
    };
    json_patch::merge(&mut tilejson2, tj);
    match serde_json::from_value(tilejson2.clone()) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to deserialize merged function comment tilejson: {e}");
            target
        }
    }
}
