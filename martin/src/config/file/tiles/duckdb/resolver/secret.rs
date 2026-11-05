//! Maps a remote `GeoParquet` source's object-store settings onto a `DuckDB` S3 secret.

use std::fmt;
use std::pin::Pin;
use std::sync::Arc;

use martin_core::tiles::duckdb::{DuckDBSecret, DuckDBSecretProvider};
use object_store::aws::{AmazonS3Builder, AmazonS3ConfigKey, AwsCredentialProvider};
use object_store::client::ClientConfigKey;
use url::Url;

use crate::config::file::ObjectStoreConfig;
use crate::config::file::object_store::is_truthy;

/// The `DuckDB` S3 secret for `url`, read from the same `AmazonS3Builder` a remote `PMTiles` source builds.
///
/// `None` for a non-`s3://` URL, or when no credentials, region, endpoint or addressing style
/// is configured: `DuckDB`'s own defaults then apply. With any credential setting (static keys,
/// a `profile`, or a credential-discovery option) the secret is re-read from the builder's
/// credential provider, so pools pick up refreshed temporary credentials; without one, instance
/// metadata is never asked. `skip_signature` drops credentials entirely.
pub(crate) fn s3_secret(
    config: &ObjectStoreConfig,
    url: &Url,
) -> Result<Option<Arc<dyn DuckDBSecretProvider>>, String> {
    if url.scheme() != "s3" {
        return Ok(None);
    }
    let bucket = url.host_str().map(str::to_owned);
    let builder = config.s3_builder(url);
    let settings = S3Settings::read(&builder, config, bucket.as_deref())?;

    if !settings.skip_signature && config.has_aws_credentials() {
        let store = builder
            .build()
            .map_err(|error| format!("unable to set up S3 credentials: {error}"))?;
        return Ok(Some(Arc::new(ProvidedSecret {
            settings,
            bucket,
            credentials: Arc::clone(store.credentials()),
        })));
    }

    let secret = settings.into_secret(bucket.as_deref());
    Ok(secret.map(|secret| Arc::new(secret) as Arc<dyn DuckDBSecretProvider>))
}

/// An S3 secret rebuilt from whatever the builder's (caching) credential provider currently holds.
struct ProvidedSecret {
    settings: S3Settings,
    bucket: Option<String>,
    credentials: AwsCredentialProvider,
}

impl fmt::Debug for ProvidedSecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProvidedSecret")
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl DuckDBSecretProvider for ProvidedSecret {
    fn secret(&self) -> Pin<Box<dyn Future<Output = Result<DuckDBSecret, String>> + Send + '_>> {
        Box::pin(async move {
            let credential = self
                .credentials
                .get_credential()
                .await
                .map_err(|error| format!("unable to load S3 credentials: {error}"))?;
            let mut settings = self.settings.clone();
            settings.key_id = Some(credential.key_id.clone());
            settings.secret = Some(credential.secret_key.clone());
            settings.session_token.clone_from(&credential.token);
            Ok(settings
                .into_secret(self.bucket.as_deref())
                .expect("a secret with a key id is never empty"))
        })
    }
}

/// The settings `DuckDB`'s S3 secret takes, as the builder resolved them.
#[derive(Clone, Default)]
struct S3Settings {
    key_id: Option<String>,
    secret: Option<String>,
    session_token: Option<String>,
    region: Option<String>,
    endpoint: Option<String>,
    virtual_hosted_style: Option<bool>,
    skip_signature: bool,
}

impl S3Settings {
    /// Reads the settings from `builder`; fails for a plain `http` endpoint without `allow_http`.
    fn read(builder: &AmazonS3Builder, config: &ObjectStoreConfig, bucket: Option<&str>) -> Result<Self, String> {
        let flag = |key: AmazonS3ConfigKey| builder.get_config_value(&key).is_some_and(|value| is_truthy(&value));
        // The builder reports an unset addressing style as `false`; only a configured one is sent.
        let virtual_hosted_style = config
            .options
            .keys()
            .any(|key| matches!(key.parse(), Ok(AmazonS3ConfigKey::VirtualHostedStyleRequest)))
            .then(|| flag(AmazonS3ConfigKey::VirtualHostedStyleRequest));
        let settings = Self {
            region: builder.get_config_value(&AmazonS3ConfigKey::Region),
            endpoint: builder
                .get_config_value(&AmazonS3ConfigKey::S3Endpoint)
                .or_else(|| builder.get_config_value(&AmazonS3ConfigKey::Endpoint)),
            virtual_hosted_style,
            skip_signature: flag(AmazonS3ConfigKey::SkipSignature),
            ..Self::default()
        };
        if let Some(endpoint) = &settings.endpoint {
            let (_, use_ssl) = duckdb_endpoint(endpoint, bucket, settings.virtual_hosted_style.unwrap_or(false));
            if use_ssl == Some(false) && !flag(AmazonS3ConfigKey::Client(ClientConfigKey::AllowHttp)) {
                return Err("plain http S3 endpoint is refused unless `allow_http` is set to `true`".to_owned());
            }
        }
        Ok(settings)
    }

    fn into_secret(self, bucket: Option<&str>) -> Option<DuckDBSecret> {
        if self.key_id.is_none()
            && self.region.is_none()
            && self.endpoint.is_none()
            && self.virtual_hosted_style.is_none()
        {
            return None;
        }

        let mut duckdb = DuckDBSecret::new("s3");
        if let Some(key_id) = self.key_id {
            duckdb = duckdb.with_str("KEY_ID", key_id);
        }
        if let Some(secret) = self.secret {
            duckdb = duckdb.with_str("SECRET", secret);
        }
        if let Some(session_token) = self.session_token {
            duckdb = duckdb.with_str("SESSION_TOKEN", session_token);
        }
        if let Some(region) = self.region {
            duckdb = duckdb.with_str("REGION", region);
        }
        if let Some(endpoint) = &self.endpoint {
            // object_store addresses a custom endpoint path-style unless told otherwise, while
            // DuckDB defaults to virtual-hosted style; keep object_store's behaviour.
            let vhost = self.virtual_hosted_style.unwrap_or(false);
            let (endpoint, use_ssl) = duckdb_endpoint(endpoint, bucket, vhost);
            duckdb = duckdb
                .with_str("ENDPOINT", endpoint)
                .with_str("URL_STYLE", if vhost { "vhost" } else { "path" });
            if let Some(use_ssl) = use_ssl {
                duckdb = duckdb.with_bool("USE_SSL", use_ssl);
            }
        } else if let Some(vhost) = self.virtual_hosted_style {
            duckdb = duckdb.with_str("URL_STYLE", if vhost { "vhost" } else { "path" });
        }
        Some(duckdb)
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
        && let Some(rest) = host.strip_prefix(bucket).and_then(|rest| rest.strip_prefix('.'))
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use object_store::CredentialProvider;
    use object_store::aws::AwsCredential;
    use rstest::rstest;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
        let provider = s3_secret(&config(values), &Url::parse(url).unwrap()).unwrap()?;
        Some(provider.secret().await.unwrap())
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

    #[rstest]
    #[case::path_style("false")]
    #[case::virtual_hosted("true")]
    #[tokio::test]
    async fn url_style_alone_makes_a_secret(#[case] vhost: &str) {
        let values = [("virtual_hosted_style_request", vhost)];
        let secret = secret(&values, "s3://bucket/a.parquet").await.unwrap();
        assert_eq!(secret.param_names().collect::<Vec<_>>(), ["URL_STYLE"]);
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

    #[test]
    fn plain_http_endpoint_needs_allow_http() {
        let url = Url::parse("s3://bucket/a.parquet").unwrap();
        let refused = config(&[("aws_endpoint", "http://localhost:9000")]);
        s3_secret(&refused, &url).unwrap_err();
        let allowed = config(&[("aws_endpoint", "http://localhost:9000"), ("allow_http", "true")]);
        assert!(s3_secret(&allowed, &url).unwrap().is_some());
    }

    #[tokio::test]
    async fn s3_endpoint_wins_over_endpoint() {
        let values = [
            ("region", "eu-west-1"),
            ("endpoint", "https://a.example"),
            ("aws_endpoint_url_s3", "https://b.example"),
        ];
        let secret = secret(&values, "s3://bucket/a.parquet").await.unwrap();
        assert_eq!(
            secret,
            DuckDBSecret::new("s3")
                .with_str("REGION", "eu-west-1")
                .with_str("ENDPOINT", "b.example")
                .with_str("URL_STYLE", "path")
                .with_bool("USE_SSL", true)
        );
    }

    #[test]
    fn static_keys_on_a_plain_http_endpoint_need_allow_http() {
        let url = Url::parse("s3://bucket/a.parquet").unwrap();
        let refused = config(&[
            ("aws_access_key_id", "id"),
            ("secret_access_key", "key"),
            ("aws_endpoint", "http://localhost:9000"),
        ]);
        insta::assert_snapshot!(s3_secret(&refused, &url).unwrap_err(), @"plain http S3 endpoint is refused unless `allow_http` is set to `true`");
    }

    /// Hands out a new key id on every call, like a profile whose temporary credentials rotate.
    #[derive(Debug, Default)]
    struct RotatingCredentials(AtomicUsize);

    #[async_trait::async_trait]
    impl CredentialProvider for RotatingCredentials {
        type Credential = AwsCredential;

        async fn get_credential(&self) -> object_store::Result<Arc<Self::Credential>> {
            let call = self.0.fetch_add(1, Ordering::SeqCst);
            Ok(Arc::new(AwsCredential {
                key_id: format!("id-{call}"),
                secret_key: "key".to_owned(),
                token: Some("token".to_owned()),
            }))
        }
    }

    #[tokio::test]
    async fn profile_secret_follows_refreshed_credentials() {
        let mut config = config(&[("region", "eu-west-1")]);
        config.aws_credentials = Some(Arc::new(RotatingCredentials::default()));
        let provider = s3_secret(&config, &Url::parse("s3://bucket/a.parquet").unwrap())
            .unwrap()
            .unwrap();

        let first = provider.secret().await.unwrap();
        let second = provider.secret().await.unwrap();
        assert_eq!(
            first.param_names().collect::<Vec<_>>(),
            ["KEY_ID", "SECRET", "SESSION_TOKEN", "REGION"]
        );
        assert_ne!(first, second);
    }

    #[tokio::test]
    async fn discovered_container_credentials_sign_the_secret() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/credentials"))
            .and(header("Authorization", "pod-token"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"AccessKeyId":"id","SecretAccessKey":"key","Token":"token","Expiration":"2100-01-01T00:00:00Z"}"#,
            ))
            // object_store caches the credentials until they expire
            .expect(1)
            .mount(&server)
            .await;
        let token_file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(token_file.path(), "pod-token").unwrap();
        let full_uri = format!("{}/credentials", server.uri());
        let config = config(&[
            ("container_credentials_full_uri", full_uri.as_str()),
            (
                "container_authorization_token_file",
                token_file.path().to_str().unwrap(),
            ),
            ("region", "eu-west-1"),
        ]);
        let provider = s3_secret(&config, &Url::parse("s3://bucket/a.parquet").unwrap())
            .unwrap()
            .unwrap();

        let first = provider.secret().await.unwrap();
        let second = provider.secret().await.unwrap();
        assert_eq!(
            first.param_names().collect::<Vec<_>>(),
            ["KEY_ID", "SECRET", "SESSION_TOKEN", "REGION"]
        );
        assert_eq!(first, second);
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
