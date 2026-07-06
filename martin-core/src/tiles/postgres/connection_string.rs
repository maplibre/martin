//! A wrapper around a `PostgreSQL` connection string that redacts its password when displayed.

use std::fmt::{self, Debug, Display};
use std::sync::LazyLock;

use regex::Regex;
use url::Url;

/// Placeholder substituted for any password found in a connection string.
const REDACTED: &str = "REDACTED";

/// A `PostgreSQL` connection string whose password is redacted for display.
///
/// `PostgreSQL` connection strings usually embed a password, either in the userinfo of a URL
/// (`postgres://user:secret@host/db`) or as a `password=` keyword/value pair.
/// Logging such a string verbatim leaks the credential, see <https://github.com/maplibre/martin/issues/2895>.
///
/// This wrapper stores only the already-redacted form, so the original password can never be
/// recovered or accidentally logged through [`Display`] or [`Debug`].
#[derive(Clone, PartialEq, Eq)]
pub struct RedactedConnectionString(String);

impl RedactedConnectionString {
    /// Redacts any password embedded in `conn_str` and wraps the result.
    #[must_use]
    pub fn new(conn_str: &str) -> Self {
        Self(redact(conn_str))
    }
}

impl From<&str> for RedactedConnectionString {
    fn from(conn_str: &str) -> Self {
        Self::new(conn_str)
    }
}

impl Display for RedactedConnectionString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Debug for RedactedConnectionString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Render as a bare quoted string, not `RedactedConnectionString("…")`.
        Debug::fmt(&self.0, f)
    }
}

/// Redacts the password in a `PostgreSQL` connection string, whichever format it is in.
fn redact(conn_str: &str) -> String {
    if let Ok(mut url) = Url::parse(conn_str) {
        if url.password().is_some() {
            let _ = url.set_password(Some(REDACTED));
        }
        // A parseable URL may still carry `?password=` in its query string.
        return redact_keyword_password(url.as_str());
    }
    // Not a parseable URL: a malformed URL or the `keyword=value` format.
    redact_keyword_password(&redact_url_userinfo(conn_str))
}

/// Redacts the `scheme://user:PASSWORD@` userinfo of a URL the `url` crate could not parse.
fn redact_url_userinfo(conn_str: &str) -> String {
    static USERINFO: LazyLock<Regex> = LazyLock::new(|| {
        // The username ends at the first `:`; the password (which may itself contain `:`) runs to `@`.
        Regex::new(r"(?i)([a-z][a-z0-9+.\-]*://[^@/?#:\s]*:)[^@/?#\s]*(@)")
            .expect("userinfo redaction regex is valid")
    });
    USERINFO
        .replace_all(conn_str, |caps: &regex::Captures| {
            format!("{}{REDACTED}{}", &caps[1], &caps[2])
        })
        .into_owned()
}

/// Redacts a `password=value` pair, as used by the `keyword=value` format and URL query strings.
fn redact_keyword_password(conn_str: &str) -> String {
    static KEYWORD: LazyLock<Regex> = LazyLock::new(|| {
        // `password = 'quoted value'` or `password=unquoted`, delimited by whitespace or `&`.
        Regex::new(r"(?i)(\bpassword\s*=\s*)('(?:[^'\\]|\\.)*'|[^\s&]*)")
            .expect("password redaction regex is valid")
    });
    KEYWORD
        .replace_all(conn_str, |caps: &regex::Captures| {
            format!("{}{REDACTED}", &caps[1])
        })
        .into_owned()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        "postgres://user:secret@localhost:5432/db",
        "postgres://user:REDACTED@localhost:5432/db"
    )]
    #[case(
        "postgres://user:p@ss:word@localhost:5432/db",
        "postgres://user:REDACTED@localhost:5432/db"
    )]
    #[case(
        "postgres://host/db?password=secret&sslmode=require",
        "postgres://host/db?password=REDACTED&sslmode=require"
    )]
    #[case(
        "host=localhost password=secret sslmode=verify-full",
        "host=localhost password=REDACTED sslmode=verify-full"
    )]
    #[case(
        "host=localhost password='se cret' dbname=db",
        "host=localhost password=REDACTED dbname=db"
    )]
    #[case(
        "postgres://user@localhost:5432/db",
        "postgres://user@localhost:5432/db"
    )]
    #[case("postgres://localhost:5432/db", "postgres://localhost:5432/db")]
    #[case("host=localhost mypassword=keep", "host=localhost mypassword=keep")]
    #[case(
        "host=localhost dbname=db sslmode=verify-full",
        "host=localhost dbname=db sslmode=verify-full"
    )]
    fn redacts(#[case] conn_str: &str, #[case] expected: &str) {
        assert_eq!(
            RedactedConnectionString::new(conn_str).to_string(),
            expected
        );
    }

    /// A URL the `url` crate cannot parse (here, an invalid port) falls back to the userinfo regex.
    #[rstest]
    #[case(
        "postgres://postgres:testpassword@host:notaport/db",
        "postgres://postgres:REDACTED@host:notaport/db"
    )]
    // A password containing `:` must be redacted whole, not just its final `:`-delimited segment.
    #[case(
        "postgres://postgres:test:password@host:notaport/db",
        "postgres://postgres:REDACTED@host:notaport/db"
    )]
    fn redacts_unparseable_url(#[case] conn_str: &str, #[case] expected: &str) {
        assert!(
            Url::parse(conn_str).is_err(),
            "test needs the regex fallback path"
        );
        assert_eq!(
            RedactedConnectionString::new(conn_str).to_string(),
            expected
        );
    }
}
