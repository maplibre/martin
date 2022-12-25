use std::ffi::OsString;

use log::warn;

/// A simple wrapper for the environment var access,
/// so we can mock it in tests.
pub trait Env {
    fn var_os(&self, key: &str) -> Option<OsString>;

    #[must_use]
    fn get_env_str(&self, name: &str) -> Option<String> {
        match self.var_os(name) {
            Some(s) => match s.into_string() {
                Ok(v) => Some(v),
                Err(v) => {
                    let v = v.to_string_lossy();
                    warn!("Environment variable {name} has invalid unicode. Lossy representation: {v}");
                    None
                }
            },
            None => None,
        }
    }
}

#[derive(Default)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        std::env::var_os(key)
    }
}
