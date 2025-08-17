// This file is included from multiple projects, so we need to make sure
// that `crate::Env` is always available, both when it is part of the lib or external to the test.
use std::ffi::OsString;

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
