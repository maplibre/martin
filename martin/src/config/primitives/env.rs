//! Environment variable access for config parsing and CLI handling.
//!
//! Provides [`Env`] trait for environment access that can be mocked in tests.
//! Substitution of `${VAR}` references inside YAML scalars is performed by
//! `serde-saphyr`'s `properties` feature; this module supplies the property map
//! and tracks which variable names appeared in the YAML so CLI argument code
//! can warn about env vars that were set but never referenced.
//!
//! - [`OsEnv`]: Production implementation
//! - [`FauxEnv`]: Test implementation

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsString;

use tracing::warn;

/// Environment variable access with Unicode validation and reference tracking.
pub trait Env {
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

    /// Build the property map handed to `serde-saphyr` for `${VAR}` interpolation.
    ///
    /// Production implementations snapshot the process environment; test fixtures
    /// return only their configured variables.
    #[must_use]
    fn as_property_map(&self) -> HashMap<String, String>;

    /// Record which variable names appeared in the YAML text just parsed.
    ///
    /// Saphyr resolves references silently against the property map, so callers
    /// pre-scan the YAML for `${VAR}` / `$VAR` tokens and feed the set here.
    /// Used by [`Env::has_unused_var`]. Default impl is a no-op for fixtures
    /// that don't need the warning UX.
    fn note_referenced(&self, _names: HashSet<String>) {}

    /// Check if an environment variable is set but was not referenced in the YAML
    /// substitution map. Returns `false` for any var that has not been observed,
    /// so callers must call [`Env::note_referenced`] before this is meaningful.
    #[must_use]
    fn has_unused_var(&self, key: &str) -> bool;
}

/// Production implementation that accesses system environment variables.
#[derive(Debug, Default)]
pub struct OsEnv(RefCell<HashSet<String>>);

impl Env for OsEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        env::var_os(key)
    }

    fn as_property_map(&self) -> HashMap<String, String> {
        env::vars().collect()
    }

    fn note_referenced(&self, names: HashSet<String>) {
        *self.0.borrow_mut() = names;
    }

    fn has_unused_var(&self, key: &str) -> bool {
        !self.0.borrow().contains(key) && env::var_os(key).is_some()
    }
}

/// Test implementation with configurable environment variables.
///
/// The tuple shape (`FauxEnv(map)`) is preserved from the pre-saphyr era so
/// existing test fixtures keep compiling. Reference-tracking is intentionally
/// omitted -- the `has_unused_var` warning is only consumed by the
/// `postgres`-CLI override path, which the unit tests don't exercise.
#[derive(Debug, Default)]
pub struct FauxEnv(pub HashMap<&'static str, OsString>);

impl FromIterator<(&'static str, OsString)> for FauxEnv {
    fn from_iter<I: IntoIterator<Item = (&'static str, OsString)>>(iter: I) -> Self {
        Self(iter.into_iter().collect())
    }
}

impl Env for FauxEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.0.get(key).map(Into::into)
    }

    fn as_property_map(&self) -> HashMap<String, String> {
        self.0
            .iter()
            .filter_map(|(k, v)| v.to_str().map(|s| ((*k).to_string(), s.to_string())))
            .collect()
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

        let env: FauxEnv = vec![("FOO", OsString::from("bar"))].into_iter().collect();
        assert_eq!(env.get_env_str("FOO"), Some("bar".to_string()));
    }

    #[test]
    #[cfg(unix)]
    fn test_bad_os_str() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt as _;

        let bad_utf8 = [0x66, 0x6f, 0x80, 0x6f];
        let os_str = OsStr::from_bytes(&bad_utf8[..]);
        let env: FauxEnv = vec![("BAD", os_str.to_owned())].into_iter().collect();
        assert!(env.0.contains_key("BAD"));
        assert_eq!(env.get_env_str("BAD"), None);
    }
}
