use std::collections::HashMap;

use postgres::types::Json;
use regex::Regex;

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
/// This is used when reporting a *malformed* connection string: because the string failed to
/// parse, it cannot be turned into a [`Config`](deadpool_postgres::tokio_postgres::Config) or a
/// [`Url`](url::Url) first, so we redact textually. Both the URL form
/// (`scheme://user:PASSWORD@host`) and the keyword form (`password=PASSWORD`) are handled; the
/// host/database/port are deliberately left intact so the operator can still tell *which*
/// connection string is at fault. If no password is found the string is returned unchanged.
#[must_use]
pub fn redact_conn_str(conn_str: &str) -> String {
    // URL form: the password sits between the first `:` of the userinfo and the `@`.
    let url_re = Regex::new(r"(?P<pre>://[^:@/?#]+:)(?P<pw>[^@/?#\s]*)(?P<at>@)")
        .expect("the regex is valid");
    let redacted = url_re.replace_all(conn_str, "${pre}****${at}");

    // Keyword form: `password=...`, optionally single- or double-quoted. Require a separator
    // (start of string, whitespace, `&` or `?`) before the keyword so we don't match a suffix
    // like `mypassword=`.
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
