//! Maps a remote `GeoParquet` source's object-store settings onto a `DuckDB` S3 secret.

use martin_core::tiles::duckdb::DuckDBSecret;
use object_store::aws::AmazonS3ConfigKey;
use object_store::client::ClientConfigKey;
use url::Url;

use crate::config::file::ObjectStoreConfig;

/// The `DuckDB` S3 secret for `url`, built from the same keys a remote `PMTiles` source takes.
///
/// `None` for a non-`s3://` URL, or when nothing is configured: `DuckDB`'s own defaults then
/// apply. Explicit `access_key_id`/`secret_access_key`/`session_token` win over credentials
/// from a `profile`, which are fetched once, here; `skip_signature` drops credentials entirely.
pub(crate) async fn s3_secret(
    config: &ObjectStoreConfig,
    url: &Url,
) -> Result<Option<DuckDBSecret>, String> {
    if url.scheme() != "s3" {
        return Ok(None);
    }

    let mut settings = S3Settings::default();
    for (key, value) in &config.options {
        let Ok(key) = key.parse::<AmazonS3ConfigKey>() else {
            continue;
        };
        settings.set(key, value);
    }

    if !settings.skip_signature
        && settings.key_id.is_none()
        && let Some(provider) = &config.aws_credentials
    {
        let credential = provider
            .get_credential()
            .await
            .map_err(|error| format!("unable to load S3 credentials from profile: {error}"))?;
        settings.key_id = Some(credential.key_id.clone());
        settings.secret = Some(credential.secret_key.clone());
        settings.session_token.clone_from(&credential.token);
    }

    settings.into_secret(url.host_str())
}

#[derive(Default)]
struct S3Settings {
    key_id: Option<String>,
    secret: Option<String>,
    session_token: Option<String>,
    region: Option<String>,
    default_region: Option<String>,
    endpoint: Option<String>,
    virtual_hosted_style: Option<bool>,
    skip_signature: bool,
    allow_http: bool,
}

impl S3Settings {
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "AmazonS3ConfigKey is non-exhaustive; other keys have no DuckDB equivalent"
    )]
    fn set(&mut self, key: AmazonS3ConfigKey, value: &str) {
        match key {
            AmazonS3ConfigKey::AccessKeyId => self.key_id = Some(value.to_owned()),
            AmazonS3ConfigKey::SecretAccessKey => self.secret = Some(value.to_owned()),
            AmazonS3ConfigKey::Token => self.session_token = Some(value.to_owned()),
            AmazonS3ConfigKey::Region => self.region = Some(value.to_owned()),
            AmazonS3ConfigKey::DefaultRegion => self.default_region = Some(value.to_owned()),
            AmazonS3ConfigKey::Endpoint | AmazonS3ConfigKey::S3Endpoint => {
                self.endpoint = Some(value.to_owned());
            }
            AmazonS3ConfigKey::VirtualHostedStyleRequest => {
                self.virtual_hosted_style = Some(is_truthy(value));
            }
            AmazonS3ConfigKey::SkipSignature => self.skip_signature = is_truthy(value),
            AmazonS3ConfigKey::Client(ClientConfigKey::AllowHttp) => {
                self.allow_http = is_truthy(value);
            }
            _ => {}
        }
    }

    fn into_secret(self, bucket: Option<&str>) -> Result<Option<DuckDBSecret>, String> {
        let region = self.region.or(self.default_region);
        let (key_id, secret, session_token) = if self.skip_signature {
            (None, None, None)
        } else {
            (self.key_id, self.secret, self.session_token)
        };
        if key_id.is_none() && region.is_none() && self.endpoint.is_none() {
            return Ok(None);
        }

        let mut duckdb = DuckDBSecret::new("s3");
        if let Some(key_id) = key_id {
            duckdb = duckdb.with_str("KEY_ID", key_id);
        }
        if let Some(secret) = secret {
            duckdb = duckdb.with_str("SECRET", secret);
        }
        if let Some(session_token) = session_token {
            duckdb = duckdb.with_str("SESSION_TOKEN", session_token);
        }
        if let Some(region) = region {
            duckdb = duckdb.with_str("REGION", region);
        }
        if let Some(endpoint) = &self.endpoint {
            // object_store addresses a custom endpoint path-style unless told otherwise, while
            // DuckDB defaults to virtual-hosted style; keep object_store's behaviour.
            let vhost = self.virtual_hosted_style.unwrap_or(false);
            let (endpoint, use_ssl) = duckdb_endpoint(endpoint, bucket, vhost);
            if use_ssl == Some(false) && !self.allow_http {
                return Err(
                    "plain http S3 endpoint is refused unless `allow_http` is set to `true`"
                        .to_owned(),
                );
            }
            duckdb = duckdb
                .with_str("ENDPOINT", endpoint)
                .with_str("URL_STYLE", if vhost { "vhost" } else { "path" });
            if let Some(use_ssl) = use_ssl {
                duckdb = duckdb.with_bool("USE_SSL", use_ssl);
            }
        } else if let Some(vhost) = self.virtual_hosted_style {
            duckdb = duckdb.with_str("URL_STYLE", if vhost { "vhost" } else { "path" });
        }
        Ok(Some(duckdb))
    }
}

/// `DuckDB`'s `ENDPOINT` (host, port and path without scheme) and `USE_SSL` for an
/// `object_store` endpoint, which is a full URL that, virtual-hosted, already names the bucket.
fn duckdb_endpoint(endpoint: &str, bucket: Option<&str>, vhost: bool) -> (String, Option<bool>) {
    let Some(url) = Url::parse(endpoint).ok().filter(Url::has_host) else {
        return (endpoint.trim_end_matches('/').to_owned(), None);
    };
    let mut host = url.host_str().unwrap_or_default();
    if vhost
        && let Some(bucket) = bucket
        && let Some(rest) = host
            .strip_prefix(bucket)
            .and_then(|rest| rest.strip_prefix('.'))
    {
        host = rest;
    }
    let mut out = host.to_owned();
    if let Some(port) = url.port() {
        out.push(':');
        out.push_str(&port.to_string());
    }
    out.push_str(url.path().trim_end_matches('/'));
    (out, Some(url.scheme() != "http"))
}

/// `object_store` keeps its boolean parser private; these are the spellings it accepts.
fn is_truthy(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "1" | "true" | "on" | "yes" | "y"
    )
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::config::file::UnrecognizedValues;

    fn config(values: &[(&str, &str)]) -> ObjectStoreConfig {
        let mut unrecognized = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), serde_json::json!(value)))
            .collect::<UnrecognizedValues>();
        let mut config = ObjectStoreConfig::default();
        config.prepare(&mut unrecognized, "duckdb");
        config
    }

    async fn secret(values: &[(&str, &str)], url: &str) -> Option<DuckDBSecret> {
        s3_secret(&config(values), &Url::parse(url).unwrap())
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn nothing_configured_leaves_duckdb_defaults() {
        assert_eq!(secret(&[], "s3://bucket/a.parquet").await, None);
    }

    #[tokio::test]
    async fn non_s3_urls_get_no_secret() {
        let values = [("aws_region", "eu-west-1")];
        assert_eq!(secret(&values, "https://host/a.parquet").await, None);
    }

    #[tokio::test]
    async fn custom_endpoint_is_path_style_without_scheme() {
        let values = [
            ("aws_access_key_id", "id"),
            ("secret_access_key", "key"),
            ("region", "auto"),
            ("aws_endpoint", "https://t3.storage.dev"),
        ];
        let secret = secret(&values, "s3://bucket/a.parquet").await.unwrap();
        insta::assert_debug_snapshot!(secret, @r#"
        DuckDBSecret {
            secret_type: "s3",
            params: [
                "KEY_ID",
                "SECRET",
                "REGION",
                "ENDPOINT",
                "URL_STYLE",
                "USE_SSL",
            ],
        }
        "#);
    }

    #[tokio::test]
    async fn skip_signature_drops_credentials() {
        let values = [
            ("aws_access_key_id", "id"),
            ("secret_access_key", "key"),
            ("region", "us-east-1"),
            ("skip_signature", "true"),
        ];
        let secret = secret(&values, "s3://bucket/a.parquet").await.unwrap();
        assert_eq!(secret.param_names().collect::<Vec<_>>(), ["REGION"]);
    }

    #[tokio::test]
    async fn plain_http_endpoint_needs_allow_http() {
        let url = Url::parse("s3://bucket/a.parquet").unwrap();
        let refused = config(&[("aws_endpoint", "http://localhost:9000")]);
        s3_secret(&refused, &url).await.unwrap_err();
        let allowed = config(&[
            ("aws_endpoint", "http://localhost:9000"),
            ("allow_http", "true"),
        ]);
        assert!(s3_secret(&allowed, &url).await.unwrap().is_some());
    }

    #[rstest]
    #[case::bare_host("t3.storage.dev", false, ("t3.storage.dev", None))]
    #[case::https("https://t3.storage.dev/", false, ("t3.storage.dev", Some(true)))]
    #[case::http_with_port("http://localhost:9000", false, ("localhost:9000", Some(false)))]
    #[case::vhost_names_bucket("https://bucket.t3.storage.dev", true, ("t3.storage.dev", Some(true)))]
    #[case::path_style_keeps_host("https://bucket.t3.storage.dev", false, ("bucket.t3.storage.dev", Some(true)))]
    fn endpoint_drops_scheme_and_bucket(
        #[case] endpoint: &str,
        #[case] vhost: bool,
        #[case] expected: (&str, Option<bool>),
    ) {
        let (endpoint, use_ssl) = duckdb_endpoint(endpoint, Some("bucket"), vhost);
        assert_eq!((endpoint.as_str(), use_ssl), expected);
    }
}
