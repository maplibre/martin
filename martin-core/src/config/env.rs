//! Environment variable access with substitution tracking.
//!
//! Provides [`Env`] trait for environment access that can be mocked in tests
//! and tracks variable usage during configuration substitution.
//!
//! - [`OsEnv`]: Production implementation
//! - [`FauxEnv`]: Test implementation

use std::cell::RefCell;
use std::collections::HashSet;
use std::env::var_os;
use std::ffi::OsString;

use log::warn;
use subst::VariableMap;

/// Environment variable access with Unicode validation and usage tracking.
///
/// Extends [`VariableMap`] to enable mocking in tests and track unused variables.
pub trait Env<'a>: VariableMap<'a> {
    /// Get an environment variable as an [`OsString`] without Unicode validation.
    fn var_os(&self, key: &str) -> Option<OsString>;

    /// Get an environment variable as a UTF-8 validated [`String`].
    ///
    /// Logs a warning and returns `None` if the variable contains invalid Unicode.
    #[must_use]
    fn get_env_str(&self, key: &str) -> Option<String> {
        match self.var_os(key) {
            Some(s) => match s.into_string() {
                Ok(v) => Some(v),
                Err(v) => {
                    let v = v.to_string_lossy();
                    warn!(
                        "Environment variable {key} has invalid unicode. Lossy representation: {v}"
                    );
                    None
                }
            },
            None => None,
        }
    }

    /// Check if an environment variable exists but was not accessed during substitution.
    #[must_use]
    fn has_unused_var(&self, key: &str) -> bool;
}

/// Production implementation that accesses system environment variables.
///
/// Tracks which variables are accessed via [`VariableMap`] using interior mutability.
#[derive(Debug, Default)]
pub struct OsEnv(RefCell<HashSet<String>>);

impl Env<'_> for OsEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        #[allow(unused_qualifications)]
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

/// Test implementation with configurable environment variables.
#[derive(Debug, Default)]
pub struct FauxEnv(pub std::collections::HashMap<&'static str, OsString>);

impl<'a> VariableMap<'a> for FauxEnv {
    type Value = String;

    fn get(&'a self, key: &str) -> Option<Self::Value> {
        self.0.get(key).map(|s| s.to_string_lossy().to_string())
    }
}

impl Env<'_> for FauxEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.0.get(key).map(Into::into)
    }

    fn has_unused_var(&self, key: &str) -> bool {
        self.var_os(key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_env_str() {
        let env = FauxEnv::default();
        assert_eq!(env.get_env_str("FOO"), None);

        let env = FauxEnv(vec![("FOO", OsString::from("bar"))].into_iter().collect());
        assert_eq!(env.get_env_str("FOO"), Some("bar".to_string()));
    }

    #[test]
    #[cfg(unix)]
    fn test_bad_os_str() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let bad_utf8 = [0x66, 0x6f, 0x80, 0x6f];
        let os_str = OsStr::from_bytes(&bad_utf8[..]);
        let env = FauxEnv(vec![("BAD", os_str.to_owned())].into_iter().collect());
        assert!(env.0.contains_key("BAD"));
        assert_eq!(env.get_env_str("BAD"), None);
    }
}
