use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::str::FromStr as _;
use std::sync::Arc;

#[cfg(test)]
use aws_config::profile::ProfileFileRegionProvider;
use aws_credential_types::provider::{ProvideCredentials as _, SharedCredentialsProvider};
#[cfg(test)]
use aws_runtime::env_config::file::EnvConfigFiles;
use dashmap::DashMap;
use object_store::aws::{AmazonS3Builder, AmazonS3ConfigKey, AwsCredential, AwsCredentialProvider};
use object_store::azure::MicrosoftAzureBuilder;
use object_store::client::{
    ClientConfigKey, ClientOptions, HttpClient, HttpConnector, ReqwestConnector,
};
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::http::HttpBuilder;
use object_store::{CredentialProvider, ObjectStore, ObjectStoreScheme};
use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use tracing::{info, warn};
use url::Url;

use crate::config::file::{CollectUnrecognizedKeys, UnrecognizedValues};
use crate::config::primitives::env::{Env, OsEnv};

const AWS_CREDENTIAL_DISCOVERY_KEYS: &[AmazonS3ConfigKey] = &[
    AmazonS3ConfigKey::ContainerCredentialsRelativeUri,
    AmazonS3ConfigKey::ContainerCredentialsFullUri,
    AmazonS3ConfigKey::ContainerAuthorizationTokenFile,
    AmazonS3ConfigKey::WebIdentityTokenFile,
    AmazonS3ConfigKey::RoleArn,
    AmazonS3ConfigKey::RoleSessionName,
    AmazonS3ConfigKey::StsEndpoint,
];

// Object-store settings shared by remote PMTiles and COG sources.
#[derive(Clone, Debug, Default, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct ObjectStoreConfig {
    /// AWS SDK profile used for S3 credentials and region resolution.
    #[serde(
        default,
        alias = "aws_profile",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub profile: Option<String>,

    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub options: HashMap<String, String>,

    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    persisted_options: BTreeMap<String, serde_json::Value>,

    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    persist_profile: bool,

    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub aws_credentials: Option<AwsCredentialProvider>,

    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub(crate) http_clients: SharedHttpClients,

    #[cfg(test)]
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub(crate) aws_profile_files: Option<EnvConfigFiles>,
}

impl Serialize for ObjectStoreConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(self.serialized_len()))?;
        self.serialize_entries(&mut map)?;
        map.end()
    }
}

fn is_sensitive_option(key: &str) -> bool {
    key.parse::<AmazonS3ConfigKey>().is_ok_and(|key| {
        matches!(
            key,
            AmazonS3ConfigKey::AccessKeyId
                | AmazonS3ConfigKey::SecretAccessKey
                | AmazonS3ConfigKey::Token
                | AmazonS3ConfigKey::ContainerAuthorizationTokenFile
                | AmazonS3ConfigKey::WebIdentityTokenFile
        ) || key.as_ref() == "aws_sse_customer_key_base64"
    }) || matches!(
        key.parse::<object_store::gcp::GoogleConfigKey>(),
        Ok(object_store::gcp::GoogleConfigKey::ServiceAccount
            | object_store::gcp::GoogleConfigKey::ServiceAccountKey
            | object_store::gcp::GoogleConfigKey::ApplicationCredentials
            | object_store::gcp::GoogleConfigKey::BearerToken)
    ) || matches!(
        key.parse::<object_store::azure::AzureConfigKey>(),
        Ok(object_store::azure::AzureConfigKey::AccessKey
            | object_store::azure::AzureConfigKey::ClientSecret
            | object_store::azure::AzureConfigKey::SasKey
            | object_store::azure::AzureConfigKey::Token
            | object_store::azure::AzureConfigKey::FederatedTokenFile
            | object_store::azure::AzureConfigKey::FabricSessionToken
            | object_store::azure::AzureConfigKey::EncryptionKey)
    )
}

fn is_proxy_option(key: &str) -> bool {
    matches!(
        key.parse::<ClientConfigKey>(),
        Ok(ClientConfigKey::ProxyUrl)
    ) || key
        .parse::<AmazonS3ConfigKey>()
        .is_ok_and(|key| key.as_ref() == "proxy_url")
        || key
            .parse::<object_store::gcp::GoogleConfigKey>()
            .is_ok_and(|key| key.as_ref() == "proxy_url")
        || key
            .parse::<object_store::azure::AzureConfigKey>()
            .is_ok_and(|key| key.as_ref() == "proxy_url")
}

fn serializable_option<'a>(
    key: &str,
    json_value: &'a serde_json::Value,
) -> Option<Cow<'a, serde_json::Value>> {
    if is_sensitive_option(key) {
        return None;
    }
    if !is_proxy_option(key) {
        return Some(Cow::Borrowed(json_value));
    }

    let serde_json::Value::String(value) = json_value else {
        return None;
    };
    let mut url = Url::parse(value).ok()?;
    if url.username().is_empty() && url.password().is_none() {
        return Some(Cow::Borrowed(json_value));
    }
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    Some(Cow::Owned(serde_json::Value::String(url.to_string())))
}

impl PartialEq for ObjectStoreConfig {
    fn eq(&self, other: &Self) -> bool {
        self.profile == other.profile && self.options == other.options
    }
}

impl ObjectStoreConfig {
    pub(crate) fn serialized_len(&self) -> usize {
        self.persisted_options
            .iter()
            .filter(|(key, value)| serializable_option(key, value).is_some())
            .count()
            + usize::from(self.persist_profile && self.profile.is_some())
    }

    pub(crate) fn serialize_entries<M>(&self, map: &mut M) -> Result<(), M::Error>
    where
        M: SerializeMap,
    {
        if self.persist_profile
            && let Some(profile) = &self.profile
        {
            map.serialize_entry("profile", profile)?;
        }
        for (key, value) in &self.persisted_options {
            if let Some(value) = serializable_option(key, value) {
                map.serialize_entry(key, &value)?;
            }
        }
        Ok(())
    }

    pub(crate) fn prepare(&mut self, unrecognized: &mut UnrecognizedValues, namespace: &str) {
        self.persist_profile = self.profile.is_some();
        self.partition_options(unrecognized, namespace);
        self.migrate_deprecated_keys(unrecognized, namespace);
    }

    pub(crate) async fn finalize_runtime(&mut self, namespace: &'static str) {
        self.import_standard_aws_env(namespace);
        self.import_aws_credential_discovery_env(&OsEnv, namespace);
        self.load_aws_profile(namespace).await;
    }

    async fn load_aws_profile(&mut self, namespace: &'static str) {
        let Some(profile) = self.profile.clone() else {
            return;
        };
        let loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .profile_name(profile.clone());
        #[cfg(test)]
        let loader = if let Some(files) = &self.aws_profile_files {
            let region_provider = ProfileFileRegionProvider::builder()
                .profile_name(profile)
                .profile_files(files.clone())
                .build();
            loader.profile_files(files.clone()).region(region_provider)
        } else {
            loader
        };
        let sdk_config = loader.load().await;
        self.apply_aws_config(&sdk_config, namespace);
    }

    fn apply_aws_config(&mut self, sdk_config: &aws_config::SdkConfig, namespace: &str) {
        let has_region = [
            "region",
            "aws_region",
            "default_region",
            "aws_default_region",
        ]
        .iter()
        .any(|key| self.options.contains_key(*key));
        if has_region {
            if namespace == "pmtiles" {
                warn!(
                    "Region from pmtiles.profile is ignored in favor of explicit PMTiles region configuration."
                );
            } else {
                warn!(
                    "Region from {namespace}.profile is ignored in favor of explicit region configuration."
                );
            }
        } else if let Some(region) = sdk_config.region() {
            self.options
                .insert("region".to_owned(), region.as_ref().to_owned());
        }

        let has_explicit_credentials = [
            "access_key_id",
            "aws_access_key_id",
            "secret_access_key",
            "aws_secret_access_key",
            "session_token",
            "aws_session_token",
            "token",
            "aws_token",
            "web_identity_token_file",
            "aws_web_identity_token_file",
            "role_arn",
            "aws_role_arn",
            "role_session_name",
            "aws_role_session_name",
            "container_credentials_relative_uri",
            "aws_container_credentials_relative_uri",
            "container_credentials_full_uri",
            "aws_container_credentials_full_uri",
            "container_authorization_token_file",
            "aws_container_authorization_token_file",
            "metadata_endpoint",
            "aws_metadata_endpoint",
            "imdsv1_fallback",
            "aws_imdsv1_fallback",
            "endpoint_url_sts",
            "aws_endpoint_url_sts",
        ]
        .iter()
        .any(|key| self.options.contains_key(*key));
        let skips_signature = ["skip_signature", "aws_skip_signature"].iter().any(|key| {
            self.options
                .get(*key)
                .is_some_and(|value| value.eq_ignore_ascii_case("true") || value == "1")
        });

        if has_explicit_credentials {
            if namespace == "pmtiles" {
                warn!(
                    "Credentials from pmtiles.profile are ignored in favor of explicit PMTiles credential-provider configuration."
                );
            } else {
                warn!(
                    "Credentials from {namespace}.profile are ignored in favor of explicit credential-provider configuration."
                );
            }
        } else if skips_signature {
            warn!(
                "Credentials from {namespace}.profile are ignored because request signing is disabled."
            );
        } else if let Some(provider) = sdk_config.credentials_provider() {
            self.aws_credentials = Some(Arc::new(AwsSdkCredentialProvider { provider }));
        }
    }

    /// Builds the store for `url`, sharing HTTP connection pools across sources.
    #[expect(
        clippy::wildcard_enum_match_arm,
        reason = "ObjectStoreScheme is non-exhaustive"
    )]
    pub(crate) fn parse_url_opts(
        &self,
        url: &Url,
    ) -> object_store::Result<(Box<dyn ObjectStore>, object_store::path::Path)> {
        macro_rules! with_options {
            ($builder:ty, $url:expr) => {
                self.options
                    .iter()
                    .fold(
                        <$builder>::new().with_url($url.to_string()),
                        |builder, (key, value)| match key.parse() {
                            Ok(key) => builder.with_config(key, value),
                            Err(_) => builder,
                        },
                    )
                    .with_http_connector(self.http_clients.clone())
            };
        }

        let (scheme, path) = ObjectStoreScheme::parse(url)?;
        let store: Box<dyn ObjectStore> = match scheme {
            ObjectStoreScheme::AmazonS3 => {
                let mut builder = with_options!(AmazonS3Builder, url);
                if let Some(credentials) = &self.aws_credentials {
                    builder = builder.with_credentials(Arc::clone(credentials));
                }
                Box::new(builder.build()?)
            }
            ObjectStoreScheme::GoogleCloudStorage => {
                Box::new(with_options!(GoogleCloudStorageBuilder, url).build()?)
            }
            ObjectStoreScheme::MicrosoftAzure => {
                Box::new(with_options!(MicrosoftAzureBuilder, url).build()?)
            }
            ObjectStoreScheme::Http => {
                let origin = &url[..url::Position::BeforePath];
                Box::new(with_options!(HttpBuilder, origin).build()?)
            }
            _ => return object_store::parse_url_opts(url, &self.options),
        };
        Ok((store, path))
    }

    fn partition_options(&mut self, unrecognized: &mut UnrecognizedValues, namespace: &str) {
        for (key, value) in unrecognized.clone() {
            let recognized = AmazonS3ConfigKey::from_str(key.as_str()).is_ok()
                || object_store::gcp::GoogleConfigKey::from_str(key.as_str()).is_ok()
                || object_store::azure::AzureConfigKey::from_str(key.as_str()).is_ok()
                || ClientConfigKey::from_str(key.as_str()).is_ok();
            if !recognized {
                continue;
            }
            unrecognized.remove(&key);
            let runtime_value = match &value {
                serde_json::Value::Bool(value) => Some(value.to_string()),
                serde_json::Value::Number(value) => Some(value.to_string()),
                serde_json::Value::String(value) => Some(value.clone()),
                value @ (serde_json::Value::Null
                | serde_json::Value::Array(_)
                | serde_json::Value::Object(_)) => {
                    warn!(
                        "Ignoring unrecognized configuration key '{namespace}.{key}': {value:?}. Only boolean, string or number values are allowed here. Please check your configuration file for typos."
                    );
                    None
                }
            };
            if let Some(runtime_value) = runtime_value {
                self.persisted_options.insert(key.clone(), value);
                self.options.insert(key, runtime_value);
            }
        }
    }

    fn migrate_deprecated_keys(&mut self, unrecognized: &mut UnrecognizedValues, namespace: &str) {
        if !self.options.contains_key("allow_http") {
            warn!(
                "Defaulting `{namespace}.allow_http` to `true`. This may become an error in the future."
            );
            self.options
                .insert("allow_http".to_owned(), "true".to_owned());
        }
        for key in ["aws_s3_force_path_style", "force_path_style"] {
            if let Some(Some(force)) = unrecognized.remove(key).map(|v| v.as_bool())
                && self.migrate_aws_value(
                    "Configuration option",
                    &format!("{namespace}.{key}"),
                    "virtual_hosted_style_request",
                    (!force).to_string(),
                    namespace,
                )
            {
                self.persisted_options.insert(
                    "virtual_hosted_style_request".to_owned(),
                    serde_json::Value::Bool(!force),
                );
            }
        }
        for key in ["aws_skip_credentials", "aws_no_credentials"] {
            if let Some(Some(skip)) = unrecognized.remove(key).map(|v| v.as_bool())
                && self.migrate_aws_value(
                    "Configuration option",
                    &format!("{namespace}.{key}"),
                    "skip_signature",
                    skip.to_string(),
                    namespace,
                )
            {
                self.persisted_options
                    .insert("skip_signature".to_owned(), serde_json::Value::Bool(skip));
            }
        }
    }

    fn import_standard_aws_env(&mut self, namespace: &str) {
        for env_key in [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "AWS_REGION",
        ] {
            if let Ok(value) = env::var(env_key) {
                let lower = env_key.to_lowercase();
                let bare = lower.strip_prefix("aws_").unwrap_or(&lower);
                self.adopt_aws_value("Environment variable", env_key, bare, value, namespace);
            }
        }
        if let Ok(profile) = env::var("AWS_PROFILE")
            && self.profile.is_none()
        {
            self.profile = Some(profile);
        }
    }

    pub(crate) fn migrate_aws_value(
        &mut self,
        kind: &str,
        key: &str,
        new_key: &str,
        value: String,
        namespace: &str,
    ) -> bool {
        let adopted = self.adopt_aws_value(kind, key, new_key, value, namespace);
        if adopted {
            warn!(
                "{kind} {key} is deprecated. Please use {namespace}.{new_key} in the configuration file instead."
            );
        }
        adopted
    }

    fn adopt_aws_value(
        &mut self,
        kind: &str,
        key: &str,
        new_key: &str,
        value: String,
        namespace: &str,
    ) -> bool {
        let prefixed = format!("aws_{new_key}");
        if self.options.contains_key(new_key) {
            warn!(
                "{kind} {key} is ignored in favor of the new configuration value {namespace}.{new_key}."
            );
            false
        } else if self.options.contains_key(&prefixed) {
            warn!(
                "{kind} {key} is ignored in favor of the new configuration value {namespace}.{prefixed}."
            );
            false
        } else {
            self.options.insert(new_key.to_owned(), value);
            true
        }
    }

    pub(crate) fn import_aws_credential_discovery_env(&mut self, env: &impl Env, namespace: &str) {
        if self.profile.is_some() {
            return;
        }
        for key in AWS_CREDENTIAL_DISCOVERY_KEYS {
            let prefixed = key.as_ref();
            let bare = prefixed.strip_prefix("aws_").unwrap_or(prefixed);
            let env_key = prefixed.to_ascii_uppercase();
            let Some(value) = env.get_env_str(&env_key) else {
                continue;
            };
            if self.options.contains_key(prefixed) || self.options.contains_key(bare) {
                continue;
            }
            info!("Using {env_key} from the environment as {namespace}.{bare} for S3 credentials.");
            self.options.insert(bare.to_owned(), value);
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn prepared(
        values: impl IntoIterator<Item = (&'static str, serde_json::Value)>,
    ) -> ObjectStoreConfig {
        let mut config = ObjectStoreConfig::default();
        let mut unrecognized = values.into_iter().collect();
        config.prepare(&mut unrecognized, "pmtiles");
        assert!(unrecognized.keys().next().is_none());
        config
    }

    #[test]
    fn saved_options_retain_their_scalar_types() {
        let config = prepared([
            ("allow_http", json!(true)),
            ("pool_max_idle_per_host", json!(7)),
            ("aws_region", json!("eu-central-1")),
        ]);

        assert_eq!(
            serde_json::to_value(config).unwrap(),
            json!({
                "allow_http": true,
                "aws_region": "eu-central-1",
                "pool_max_idle_per_host": 7,
            })
        );
    }

    #[test]
    fn saved_options_redact_s3_customer_keys_and_proxy_credentials() {
        let config = prepared([
            ("aws_sse_customer_key_base64", json!("first-secret")),
            ("sse_customer_key_base64", json!("second-secret")),
        ]);
        assert_eq!(serde_json::to_value(config).unwrap(), json!({}));

        for key in [
            "proxy_url",
            "aws_proxy_url",
            "google_proxy_url",
            "azure_proxy_url",
        ] {
            let config = prepared([(
                key,
                json!("http://proxy-user:proxy-password@proxy.example.com:8080"),
            )]);
            assert_eq!(
                serde_json::to_value(config).unwrap(),
                json!({key: "http://proxy.example.com:8080/"})
            );
        }
    }

    #[test]
    fn migrated_boolean_options_remain_booleans_when_saved() {
        let config = prepared([
            ("aws_s3_force_path_style", json!(true)),
            ("aws_skip_credentials", json!(false)),
        ]);

        assert_eq!(
            serde_json::to_value(config).unwrap(),
            json!({
                "skip_signature": false,
                "virtual_hosted_style_request": false,
            })
        );
    }
}

/// Shared object-store HTTP client cache.
#[derive(Clone, Debug, Default)]
pub struct SharedHttpClients(pub(crate) Arc<DashMap<String, HttpClient>>);

impl HttpConnector for SharedHttpClients {
    fn connect(&self, options: &ClientOptions) -> object_store::Result<HttpClient> {
        let client = self
            .0
            .entry(format!("{options:?}"))
            .or_try_insert_with(|| ReqwestConnector::default().connect(options))?;
        Ok(client.clone())
    }
}

#[derive(Debug)]
pub struct AwsSdkCredentialProvider {
    provider: SharedCredentialsProvider,
}

#[async_trait::async_trait]
impl CredentialProvider for AwsSdkCredentialProvider {
    type Credential = AwsCredential;

    async fn get_credential(&self) -> object_store::Result<Arc<Self::Credential>> {
        let credentials = self
            .provider
            .provide_credentials()
            .await
            .map_err(|source| object_store::Error::Generic {
                store: "S3",
                source: Box::new(source),
            })?;
        Ok(Arc::new(AwsCredential {
            key_id: credentials.access_key_id().to_owned(),
            secret_key: credentials.secret_access_key().to_owned(),
            token: credentials.session_token().map(str::to_owned),
        }))
    }
}

#[cfg(all(test, feature = "unstable-cog"))]
mod cog_tests {
    use object_store::ObjectStoreExt as _;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use crate::config::file::cog::CogConfig;
    use crate::config::file::{Config, ConfigurationLivecycleHooks as _, FileConfigEnum};
    use crate::config::primitives::IdResolver;
    use crate::srv::RESERVED_KEYWORDS;

    #[tokio::test]
    async fn serialization_preserves_connection_options_and_redacts_credentials() {
        let mut config: CogConfig = serde_saphyr::from_str(
            "aws_endpoint: http://localhost:9000\naws_region: us-east-1\nallow_http: true\nskip_signature: false\naws_access_key_id: secret-id\nsecret_access_key: secret-key\ngoogle_bearer_token: secret-token\nazure_storage_sas_key: secret-sas\n",
        )
        .unwrap();
        config.finalize().await.unwrap();

        let value = serde_json::to_value(config).unwrap();
        insta::assert_json_snapshot!(value, @r#"
        {
          "allow_http": true,
          "aws_endpoint": "http://localhost:9000",
          "aws_region": "us-east-1",
          "skip_signature": false
        }
        "#);
    }

    #[tokio::test]
    async fn connection_options_survive_source_resolution() {
        let mut config: Config = serde_saphyr::from_str(
            "on_invalid: warn\ncog:\n  aws_endpoint: http://localhost:9000\n  allow_http: true\n  skip_signature: true\n  sources:\n    local: tests/fixtures/cog/usda_naip_128_none_z2.tif\n",
        )
        .unwrap();
        config.finalize().await.unwrap();
        let FileConfigEnum::Config(cog) = &config.cog else {
            panic!("COG config must be expanded during finalization");
        };
        assert_eq!(
            cog.custom.object_store.options["aws_endpoint"],
            "http://localhost:9000"
        );

        let state = config
            .resolve(&IdResolver::new(RESERVED_KEYWORDS))
            .await
            .unwrap();
        let saved = serde_saphyr::to_string(&config.with_catalog(&state.tile_manager)).unwrap();
        let saved: serde_json::Value = serde_saphyr::from_str(&saved).unwrap();
        insta::assert_json_snapshot!(saved["cog"], @r#"
        {
          "allow_http": true,
          "aws_endpoint": "http://localhost:9000",
          "skip_signature": true
        }
        "#);
    }

    #[tokio::test]
    async fn generic_http_store_can_head_an_object() {
        let server = MockServer::start().await;
        Mock::given(method("HEAD"))
            .and(path("/image.tif"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-length", "42")
                    .insert_header("etag", "\"fixture\""),
            )
            .mount(&server)
            .await;

        let mut cog = CogConfig::default();
        cog.finalize().await.unwrap();
        let config = &cog.object_store;
        let url = url::Url::parse(&format!("{}/image.tif", server.uri())).unwrap();
        let (store, object_path) = config.parse_url_opts(&url).unwrap();
        let metadata = store.head(&object_path).await.unwrap();

        assert_eq!(metadata.size, 42);
        assert_eq!(metadata.e_tag.as_deref(), Some("\"fixture\""));
    }
}
