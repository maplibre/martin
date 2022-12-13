use crate::pg::utils::PgError;
use crate::pmtiles::utils::PmtError;
use itertools::Itertools;
use log::{error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env::VarError;
use std::path::PathBuf;
use std::{env, io};

pub type InfoMap<T> = HashMap<String, T>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Unable to load config file {}: {0}", .1.display())]
    ConfigLoadError(io::Error, PathBuf),

    #[error("Unable to parse config file {}: {0}", .1.display())]
    ConfigParseError(serde_yaml::Error, PathBuf),

    #[error("Unable to write config file {}: {0}", .1.display())]
    ConfigWriteError(io::Error, PathBuf),

    #[error("{0}")]
    PostgresError(#[from] PgError),

    #[error("{0}")]
    PmtilesError(#[from] PmtError),
}

pub type Result<T> = std::result::Result<T, Error>;

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

/// A list of schemas to include in the discovery process, or a boolean to
/// indicate whether to run discovery at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Schemas {
    Bool(bool),
    List(Vec<String>),
}

impl Schemas {
    /// Returns a list of schemas to include in the discovery process.
    /// If self is a true, returns a list of all schemas produced by the callback.
    pub fn get<'a, I, F>(&self, keys: F) -> Vec<String>
    where
        I: Iterator<Item = &'a String>,
        F: FnOnce() -> I,
    {
        match self {
            Schemas::List(lst) => lst.clone(),
            Schemas::Bool(all) => {
                if *all {
                    keys().sorted().map(String::to_string).collect()
                } else {
                    Vec::new()
                }
            }
        }
    }
}

pub fn get_env_str(name: &str) -> Option<String> {
    match env::var(name) {
        Ok(v) => Some(v),
        Err(VarError::NotPresent) => None,
        Err(VarError::NotUnicode(v)) => {
            let v = v.to_string_lossy();
            warn!("Environment variable {name} has invalid unicode. Lossy representation: {v}");
            None
        }
    }
}
