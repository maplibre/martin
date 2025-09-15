use std::collections::{BTreeMap, HashMap};

use deadpool_postgres::tokio_postgres::types::Json;
use itertools::Itertools as _;
use log::{error, info, warn};
use martin_core::tiles::UrlQuery;

#[must_use]
pub fn query_to_json(query: Option<&UrlQuery>) -> Json<HashMap<String, serde_json::Value>> {
    let mut query_as_json = HashMap::new();
    if let Some(query) = query {
        for (k, v) in query {
            let json_value: serde_json::Value =
                serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone()));

            query_as_json.insert(k.clone(), json_value);
        }
    }

    Json(query_as_json)
}

#[must_use]
pub fn normalize_key<T>(
    map: &BTreeMap<String, T>,
    key: &str,
    info: &str,
    id: &str,
) -> Option<String> {
    find_info_kv(map, key, info, id).map(|(k, _)| k.to_string())
}

#[must_use]
pub fn find_info<'a, T>(
    map: &'a BTreeMap<String, T>,
    key: &'a str,
    info: &str,
    id: &str,
) -> Option<&'a T> {
    find_info_kv(map, key, info, id).map(|(_, v)| v)
}

#[must_use]
fn find_info_kv<'a, T>(
    map: &'a BTreeMap<String, T>,
    key: &'a str,
    info: &str,
    id: &str,
) -> Option<(&'a str, &'a T)> {
    if let Some(v) = map.get(key) {
        return Some((key, v));
    }

    match find_kv_ignore_case(map, key) {
        Ok(None) => {
            warn!(
                "Unable to configure source {id} because {info} '{key}' was not found.  Possible values are: {}",
                map.keys().map(String::as_str).join(", ")
            );
            None
        }
        Ok(Some(result)) => {
            info!("For source {id}, {info} '{key}' was not found, but found '{result}' instead.");
            Some((result.as_str(), map.get(result)?))
        }
        Err(multiple) => {
            error!(
                "Unable to configure source {id} because {info} '{key}' has no exact match and more than one potential matches: {}",
                multiple.join(", ")
            );
            None
        }
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
                        multiple.push(result.to_string());
                    }
                    multiple.push(k.to_string());
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
