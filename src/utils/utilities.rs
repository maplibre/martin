use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use log::{error, info, warn};

use crate::pg::PgError;

pub type InfoMap<T> = HashMap<String, T>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("The --config and the connection parameters cannot be used together")]
    ConfigAndConnectionsError,

    #[error("Unable to load config file {}: {0}", .1.display())]
    ConfigLoadError(io::Error, PathBuf),

    #[error("Unable to parse config file {}: {0}", .1.display())]
    ConfigParseError(subst::yaml::Error, PathBuf),

    #[error("Unable to write config file {}: {0}", .1.display())]
    ConfigWriteError(io::Error, PathBuf),

    #[error("{0}")]
    PostgresError(#[from] PgError),
}

pub type Result<T> = std::result::Result<T, Error>;

#[must_use]
pub fn normalize_key<'a, T>(
    map: &'a InfoMap<T>,
    key: &str,
    info: &str,
    id: &str,
) -> Option<String> {
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
            info!("For source {id}, {info} '{key}' was not found, using '{result}' instead.");
            Some((result.as_str(), map.get(result)?))
        } else {
            warn!("Unable to configure source {id} because {info} '{key}' was not found.  Possible values are: {}",
                map.keys().map(String::as_str).collect::<Vec<_>>().join(", "));
            None
        }
    } else {
        error!("Unable to configure source {id} because {info} '{key}' has no exact match and more than one potential matches: {}", multiple.join(", "));
        None
    }
}

/// Update empty option in place with a non-empty value from the second option.
pub fn set_option<T>(first: &mut Option<T>, second: Option<T>) {
    if first.is_none() && second.is_some() {
        *first = second;
    }
}

#[must_use]
pub fn is_valid_zoom(zoom: i32, minzoom: Option<u8>, maxzoom: Option<u8>) -> bool {
    minzoom.map_or(true, |minzoom| zoom >= minzoom.into())
        && maxzoom.map_or(true, |maxzoom| zoom <= maxzoom.into())
}
