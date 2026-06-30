use std::collections::HashMap;
use std::env;
use std::ffi::OsString;

use tracing::warn;

pub trait Env {
    fn var_os(&self, key: &str) -> Option<OsString>;

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

    #[must_use]
    fn as_property_map(&self) -> HashMap<String, String>;
}

#[derive(Debug, Default)]
pub struct OsEnv;

impl Env for OsEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        env::var_os(key)
    }

    fn as_property_map(&self) -> HashMap<String, String> {
        env::vars().collect()
    }
}

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
