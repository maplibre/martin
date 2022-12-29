use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use tilejson::{Bounds, TileJSON, VectorLayer};

use crate::pg::PgError;

pub type InfoMap<T> = HashMap<String, T>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("The --config and the connection parameters cannot be used together")]
    ConfigAndConnectionsError,

    #[error("Unable to bind to {1}: {0}")]
    BindingError(io::Error, String),

    #[error("Unable to load config file {}: {0}", .1.display())]
    ConfigLoadError(io::Error, PathBuf),

    #[error("Unable to parse config file {}: {0}", .1.display())]
    ConfigParseError(subst::yaml::Error, PathBuf),

    #[error("Unable to write config file {}: {0}", .1.display())]
    ConfigWriteError(io::Error, PathBuf),

    #[error("No tile sources found. Set sources by giving a database connection string on command line, env variable, or a config file.")]
    NoSources,

    #[error("Unrecognizable connection strings: {0:?}")]
    UnrecognizableConnections(Vec<String>),

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
pub fn is_valid_zoom(zoom: i32, minzoom: Option<u8>, maxzoom: Option<u8>) -> bool {
    minzoom.map_or(true, |minzoom| zoom >= minzoom.into())
        && maxzoom.map_or(true, |maxzoom| zoom <= maxzoom.into())
}

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BoolOrObject<T> {
    Bool(bool),
    Object(T),
}

#[must_use]
pub fn create_tilejson(
    name: String,
    minzoom: Option<u8>,
    maxzoom: Option<u8>,
    bounds: Option<Bounds>,
    vector_layers: Option<Vec<VectorLayer>>,
) -> TileJSON {
    let mut tilejson = tilejson::tilejson! {
        tilejson: "2.2.0".to_string(),
        tiles: vec![],  // tile source is required, but not yet known
        name: name,
    };
    tilejson.minzoom = minzoom;
    tilejson.maxzoom = maxzoom;
    tilejson.bounds = bounds;
    tilejson.vector_layers = vector_layers;

    // TODO: consider removing - this is not needed per TileJSON spec
    tilejson.set_missing_defaults();
    tilejson
}
