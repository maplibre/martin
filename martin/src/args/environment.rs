use std::cell::RefCell;
use std::collections::HashSet;
use std::env::var_os;
use std::ffi::OsString;

use log::warn;
use subst::VariableMap;

/// A simple wrapper for the environment var access,
/// so we can mock it in tests.
pub trait Env<'a>: VariableMap<'a> {
    fn var_os(&self, key: &str) -> Option<OsString>;

    #[must_use]
    fn get_env_str(&self, key: &str) -> Option<String> {
        match self.var_os(key) {
            Some(s) => {
                match s.into_string() {
                    Ok(v) => Some(v),
                    Err(v) => {
                        let v = v.to_string_lossy();
                        warn!("Environment variable {key} has invalid unicode. Lossy representation: {v}");
                        None
                    }
                }
            }
            None => None,
        }
    }

    /// Return true if the environment variable exists, and it was no used by the substitution process.
    #[must_use]
    fn has_unused_var(&self, key: &str) -> bool;
}

/// A map that gives strings from the environment,
/// but also keeps track of which variables were requested via the `VariableMap` trait.
#[derive(Default)]
pub struct OsEnv(RefCell<HashSet<String>>);

impl<'a> Env<'a> for OsEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }

    fn has_unused_var(&self, key: &str) -> bool {
        !self.0.borrow().contains(key) && var_os(key).is_some()
    }
}

impl<'a> VariableMap<'a> for OsEnv {
    type Value = String;

    fn get(&'a self, key: &str) -> Option<Self::Value> {
        self.0.borrow_mut().insert(key.to_string());
        std::env::var(key).ok()
    }
}
