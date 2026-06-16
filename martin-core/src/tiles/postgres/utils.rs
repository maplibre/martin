use std::collections::HashMap;

use postgres::types::Json;
use regex::Regex;
use url::Url;

use crate::tiles::UrlQuery;

/// Converts a `UrlQuery` into a semantically identical `Json<HashMap<String, serde_json::Value>>`.
#[must_use]
pub fn query_to_json(query: Option<&UrlQuery>) -> Json<HashMap<String, serde_json::Value>> {
    let mut query_as_json = HashMap::new();
    if let Some(query) = query {
        for (k, v) in query {
            let json_value: serde_json::Value =
                serde_json::from_str(v).unwrap_or_else(|_| serde_json::Value::String(v.clone()));

            query_as_json.insert(k.clone(), json_value);
        }
    }

    Json(query_as_json)
}

/// Best-effort redaction of the password in a `PostgreSQL` connection string.
///
/// Used when reporting a *malformed* connection string (one that failed to parse as a Postgres
/// [`Config`](deadpool_postgres::tokio_postgres::Config)). For the URL form we parse with the
/// [`url`](url::Url) crate and rewrite the password - a failed `Config` parse is usually still a
/// valid URL. The keyword form (`password=PASSWORD`) and any URL `url` can't parse fall back to
/// textual regex redaction. The host/database/port are deliberately left intact so the operator
/// can still tell *which* connection string is at fault; if no password is found the string is
/// returned unchanged.
#[must_use]
pub fn redact_conn_str(conn_str: &str) -> String {
    // Prefer `url` crate for the URL form. We only get here for strings that failed to
    // parse as a Postgres `Config`, but most of those are still valid URLs.
    if let Ok(mut url) = Url::parse(conn_str)
        && url.password().is_some()
        && url.set_password(Some("****")).is_ok()
    {
        return url.into();
    }

    // Fallback for what `url` can't handle: the libpq keyword form (`host=... password=...`),
    // which isn't a URL, plus the rare malformed URL that `url` rejects outright. Best-effort
    // textual redaction of both the URL userinfo and a `password=` keyword.
    let url_re = Regex::new(r"(?P<pre>://[^:@/?#]+:)(?P<pw>[^@/?#\s]*)(?P<at>@)")
        .expect("the regex is valid");
    let redacted = url_re.replace_all(conn_str, "${pre}****${at}");

    // `password=...`, optionally single- or double-quoted. Require a separator (start of string,
    // whitespace, `&` or `?`) before the keyword so we don't match a suffix like `mypassword=`.
    let kw_re = Regex::new(r#"(?P<pre>(?:^|[\s&?])password=)(?:'[^']*'|"[^"]*"|[^\s&]*)"#)
        .expect("the regex is valid");
    kw_re.replace_all(&redacted, "${pre}****").into_owned()
}

#[cfg(test)]
mod tests {
    use super::redact_conn_str;

    #[test]
    fn redacts_url_password() {
        let redacted = redact_conn_str("postgres://user:secret@localhost:5432/db");
        assert_eq!(redacted, "postgres://user:****@localhost:5432/db");
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn redacts_malformed_url_password() {
        // The original bug report: gibberish after the host makes the string unparseable,
        // but the password must still be hidden.
        let redacted = redact_conn_str(
            "postgres://postgres:testpassword@host.docke???WQD?wq/db:5432/database",
        );
        assert!(!redacted.contains("testpassword"));
        assert!(redacted.starts_with("postgres://postgres:****@host.docke"));
    }

    #[test]
    fn redacts_url_password_containing_at_and_colon() {
        // The `url` crate delimits the userinfo at the *last* `@`, so a password that itself
        // contains `@`/`:` is fully hidden (a naive regex would stop at the first `@`).
        let redacted = redact_conn_str("postgres://user:p@ss:word@localhost:5432/db");
        assert_eq!(redacted, "postgres://user:****@localhost:5432/db");
        assert!(!redacted.contains("ss:word"));
    }

    #[test]
    fn keeps_url_without_password() {
        let conn = "postgres://user@localhost:5432/db";
        assert_eq!(redact_conn_str(conn), conn);
    }

    #[test]
    fn does_not_redact_host_port() {
        // No userinfo: the `host:port` colon must not be mistaken for a password separator.
        let conn = "postgres://localhost:5432/db";
        assert_eq!(redact_conn_str(conn), conn);
    }

    #[test]
    fn redacts_keyword_password() {
        let redacted = redact_conn_str("host=localhost password=secret dbname=db");
        assert_eq!(redacted, "host=localhost password=**** dbname=db");
        assert!(!redacted.contains("secret"));
    }

    #[test]
    fn redacts_quoted_keyword_password() {
        let redacted = redact_conn_str("host=localhost password='se cret' dbname=db");
        assert_eq!(redacted, "host=localhost password=**** dbname=db");
        assert!(!redacted.contains("se cret"));
    }

    #[test]
    fn does_not_redact_password_suffix_keyword() {
        // `mypassword=` is a different key and has no secret to hide.
        let conn = "host=localhost mypassword=keep";
        assert_eq!(redact_conn_str(conn), conn);
    }

    #[test]
    fn leaves_credential_free_string_untouched() {
        let conn = "host=localhost dbname=db";
        assert_eq!(redact_conn_str(conn), conn);
    }
}
