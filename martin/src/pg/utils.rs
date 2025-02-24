use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::time::Duration;

use deadpool_postgres::tokio_postgres::types::Json;
use futures::pin_mut;
use itertools::Itertools as _;
use log::{error, info, warn};
use postgis::{LineString, Point, Polygon, ewkb};
use tilejson::{Bounds, TileJSON};
use tokio::time::timeout;

use crate::source::UrlQuery;

#[cfg(test)]
#[expect(clippy::ref_option)]
pub fn sorted_opt_set<S: serde::Serializer>(
    value: &Option<std::collections::HashSet<String>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use serde::Serialize as _;

    value
        .as_ref()
        .map(|v| {
            let mut v: Vec<_> = v.iter().collect();
            v.sort();
            v
        })
        .serialize(serializer)
}

pub async fn on_slow<T, S: FnOnce()>(
    future: impl Future<Output = T>,
    duration: Duration,
    fn_on_slow: S,
) -> T {
    pin_mut!(future);
    if let Ok(result) = timeout(duration, &mut future).await {
        result
    } else {
        fn_on_slow();
        future.await
    }
}

#[must_use]
pub fn json_to_hashmap(value: &serde_json::Value) -> InfoMap<String> {
    let mut result = BTreeMap::new();

    let object = value.as_object().unwrap();
    for (key, value) in object {
        let string_value = value.as_str().unwrap().to_string();
        result.insert(key.clone(), string_value);
    }

    result
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
pub fn polygon_to_bbox(polygon: &ewkb::Polygon) -> Option<Bounds> {
    polygon.rings().next().and_then(|linestring| {
        let mut points = linestring.points();
        if let (Some(bottom_left), Some(top_right)) = (points.next(), points.nth(1)) {
            Some(Bounds::new(
                bottom_left.x(),
                bottom_left.y(),
                top_right.x(),
                top_right.y(),
            ))
        } else {
            None
        }
    })
}

pub type InfoMap<T> = BTreeMap<String, T>;

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
/// If there is no exact match, but there is a case-insensitive match, return that as `Ok(Some(value))`.
/// If there is no exact match and there are multiple case-insensitive matches, return an error with a vector of the possible matches.
/// If there is no match, return `Ok(None)`.
pub fn find_kv_ignore_case<'a, T>(
    map: &'a InfoMap<T>,
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
