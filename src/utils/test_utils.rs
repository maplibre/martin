// This file is included from multiple projects, so we need to make sure
// that `crate::Env` is always available, both when it is part of the lib or external to the test.
use std::ffi::OsString;

use crate::Env;

#[allow(clippy::unnecessary_wraps)]
#[must_use]
pub fn some_str(s: &str) -> Option<String> {
    Some(s.to_string())
}

#[derive(Default)]
pub struct FauxEnv(pub std::collections::HashMap<&'static str, OsString>);

impl Env for FauxEnv {
    fn var_os(&self, key: &str) -> Option<OsString> {
        self.0.get(key).map(Into::into)
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
