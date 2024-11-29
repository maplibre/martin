// This file is included from multiple projects, so we need to make sure
// that `crate::Env` is always available, both when it is part of the lib or external to the test.
use std::ffi::OsString;

use subst::VariableMap;

use crate::Env;

#[allow(clippy::unnecessary_wraps)]
#[must_use]
pub fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

#[allow(clippy::unnecessary_wraps)]
#[must_use]
pub fn os(s: &str) -> OsString {
    OsString::from(s)
}

#[derive(Default)]
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

        let env = FauxEnv(vec![("FOO", os("bar"))].into_iter().collect());
        assert_eq!(env.get_env_str("FOO"), some("bar"));
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
