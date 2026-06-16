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
    use rstest::rstest;

    use super::redact_conn_str;

    #[rstest]
    // URL form: only the password between `:` and `@` is replaced; the rest is preserved.
    #[case::url_password(
        "postgres://user:secret@localhost:5432/db",
        "postgres://user:****@localhost:5432/db"
    )]
    // The original bug report: gibberish after the host makes the string fail to parse as a
    // Postgres `Config`, but it's still a valid URL, so the password is still hidden.
    #[case::malformed_url(
        "postgres://postgres:testpassword@host.docke???WQD?wq/db:5432/database",
        "postgres://postgres:****@host.docke???WQD?wq/db:5432/database"
    )]
    // Password containing `@`/`:`: `url` delimits the userinfo at the *last* `@`, hiding it fully
    // (a naive regex would stop at the first `@` and leak the rest).
    #[case::password_with_at_and_colon(
        "postgres://user:p@ss:word@localhost:5432/db",
        "postgres://user:****@localhost:5432/db"
    )]
    // No password to hide: returned unchanged.
    #[case::url_without_password(
        "postgres://user@localhost:5432/db",
        "postgres://user@localhost:5432/db"
    )]
    // No userinfo: the `host:port` colon must not be mistaken for a password separator.
    #[case::host_port_only("postgres://localhost:5432/db", "postgres://localhost:5432/db")]
    // libpq keyword form.
    #[case::keyword_password(
        "host=localhost password=secret dbname=db",
        "host=localhost password=**** dbname=db"
    )]
    // Quoted keyword value (may contain spaces).
    #[case::quoted_keyword_password(
        "host=localhost password='se cret' dbname=db",
        "host=localhost password=**** dbname=db"
    )]
    // `mypassword=` is a different key, not a secret to hide.
    #[case::password_suffix_keyword(
        "host=localhost mypassword=keep",
        "host=localhost mypassword=keep"
    )]
    // Nothing credential-like: unchanged.
    #[case::no_credentials("host=localhost dbname=db", "host=localhost dbname=db")]
    fn redacts_password(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(redact_conn_str(input), expected);
    }
}
