use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::str::FromStr as _;
use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
use aws_config::profile::ProfileFileRegionProvider;
use aws_credential_types::provider::{ProvideCredentials as _, SharedCredentialsProvider};
#[cfg(test)]
use aws_runtime::env_config::file::EnvConfigFiles;
use dashmap::DashMap;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesSource};
use object_store::aws::{AmazonS3Builder, AmazonS3ConfigKey, AwsCredential, AwsCredentialProvider};
use object_store::azure::MicrosoftAzureBuilder;
use object_store::client::{ClientOptions, HttpClient, HttpConnector, ReqwestConnector};
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::http::HttpBuilder;
use object_store::{CredentialProvider, ObjectStore, ObjectStoreScheme};
use serde::{Deserialize, Serialize};
use tracing::{info, trace, warn};
use url::Url;

use crate::config::file::{
    CachePolicy, CacheSizeConfig, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    ConfigurationLivecycleHooks, SourceBuildResult, TileSourceConfiguration, UnrecognizedValues,
};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};
use crate::config::primitives::env::{Env, OsEnv};

/// Default polling interval for [`PmtilesReloader`](crate::config::file::reload::pmtiles::PmtilesReloader)
/// to re-list remote URL prefixes (s3://, gs://, https://, etc.). Local directories are
/// notify-driven and ignore this setting.
pub const DEFAULT_RELOAD_INTERVAL: Duration = Duration::from_mins(10);

/// `object_store` options that AWS runtimes set through the environment to say where credentials
/// come from: ECS/Fargate task roles, EKS IRSA and EKS Pod Identity.
///
/// The variable name is the option name upper-cased, which is also how
/// [`AmazonS3Builder::from_env`] reads them. The values are per task (a random relative URI, a
/// rotating token path), so they cannot be written into a config file ahead of time the way keys
/// or a profile can.
const AWS_CREDENTIAL_DISCOVERY_KEYS: &[AmazonS3ConfigKey] = &[
    AmazonS3ConfigKey::ContainerCredentialsRelativeUri,
    AmazonS3ConfigKey::ContainerCredentialsFullUri,
    AmazonS3ConfigKey::ContainerAuthorizationTokenFile,
    AmazonS3ConfigKey::WebIdentityTokenFile,
    AmazonS3ConfigKey::RoleArn,
    AmazonS3ConfigKey::RoleSessionName,
    AmazonS3ConfigKey::StsEndpoint,
];

fn default_reload_interval() -> Duration {
    DEFAULT_RELOAD_INTERVAL
}

fn is_default_reload_interval(v: &Duration) -> bool {
    *v == DEFAULT_RELOAD_INTERVAL
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct PmtConfig {
    /// Size of the directory cache (in MB).
    /// Defaults to `cache.size_mb` / 4
    ///
    /// Note:
    /// Tile and directory caching are complementary.
    /// For good performance, you want
    /// - directory caching (to not resolve the directory on each request) and
    /// - tile caching (for high access tiles)
    ///
    /// Use `directory_cache: disable` to disable
    #[serde(default, skip_serializing_if = "CacheSizeConfig::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CacheSizeConfigShape")
    )]
    pub directory_cache: CacheSizeConfig,

    /// How often remote URL prefixes (`s3://bucket/`, `gs://bucket/`, etc.) re-`LIST` for source discovery.
    /// Has no effect on local directories, which are watched via filesystem events.
    ///
    /// Supports human-readable formats: "10m", "1h", "30s".
    /// Defaults to "10m". Set to "0s" to disable remote polling.
    #[serde(
        default = "default_reload_interval",
        skip_serializing_if = "is_default_reload_interval",
        with = "humantime_serde"
    )]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "String", example = &"10m")
    )]
    pub reload_interval: Duration,

    /// AWS SDK profile used for S3 credentials and region resolution.
    #[serde(
        default,
        alias = "aws_profile",
        skip_serializing_if = "Option::is_none"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub profile: Option<String>,

    // if the key is the allowed set, we assume it is there for a purpose
    // settings and unreconginsed values are partitioned from each other in the init_parsing step
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub options: HashMap<String, String>,

    /// MVT->MLT encoder settings for all `PMTiles` sources.
    /// Overrides global; overridden by per-source `convert_to_mlt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,

    /// MLT->MVT conversion settings for all `PMTiles` sources.
    /// Overrides global; overridden by per-source `convert_to_mvt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_mvt: Option<MvtProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,

    /// `PMTiles` directory cache (internal state, not serialized)
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub pmtiles_directory_cache: PmtCache,

    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub aws_credentials: Option<AwsCredentialProvider>,

    /// HTTP clients shared by every remote source of this config (internal state, not serialized)
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub(crate) http_clients: SharedHttpClients,

    #[cfg(test)]
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub(crate) aws_profile_files: Option<EnvConfigFiles>,
}

impl Default for PmtConfig {
    fn default() -> Self {
        Self {
            directory_cache: CacheSizeConfig::default(),
            reload_interval: DEFAULT_RELOAD_INTERVAL,
            profile: None,
            options: HashMap::default(),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: None,
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: None,
            unrecognized: UnrecognizedValues::default(),
            pmtiles_directory_cache: PmtCache::default(),
            aws_credentials: None,
            http_clients: SharedHttpClients::default(),
            #[cfg(test)]
            aws_profile_files: None,
        }
    }
}

impl PartialEq for PmtConfig {
    fn eq(&self, other: &Self) -> bool {
        let base = self.directory_cache == other.directory_cache
            && self.reload_interval == other.reload_interval
            && self.profile == other.profile
            && self.options == other.options
            && self.unrecognized == other.unrecognized;
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        let base = base
            && self.convert_to_mlt == other.convert_to_mlt
            && self.convert_to_mvt == other.convert_to_mvt;
        // pmtiles_directory_cache and http_clients are intentionally excluded from equality check
        base
    }
}

impl ConfigurationLivecycleHooks for PmtConfig {
    async fn finalize(&mut self) -> ConfigFileResult<()> {
        // if the key is the allowed set, we assume it is there for a purpose
        // because of how serde(flatten) works, we need to collect all in one place and then
        // partition them into options and unrecognized keys
        //
        // If we don't do this, the error message is not clear enough
        self.partition_options_and_unrecognized();
        self.migrate_deprecated_keys();
        self.import_aws_credential_discovery_env(&OsEnv);
        self.load_aws_profile().await;

        Ok(())
    }
}

impl PmtConfig {
    async fn load_aws_profile(&mut self) {
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
        self.apply_aws_config(&sdk_config);
    }

    fn apply_aws_config(&mut self, sdk_config: &aws_config::SdkConfig) {
        let region_specified_by_config = [
            "region",
            "aws_region",
            "default_region",
            "aws_default_region",
        ]
        .iter()
        .any(|key| self.options.contains_key(*key));
        if region_specified_by_config {
            warn!(
                "Region from pmtiles.profile is ignored in favor of explicit PMTiles region configuration."
            );
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
            warn!(
                "Credentials from pmtiles.profile are ignored in favor of explicit PMTiles credential-provider configuration."
            );
        } else if skips_signature {
            warn!(
                "Credentials from pmtiles.profile are ignored because request signing is disabled."
            );
        } else if let Some(provider) = sdk_config.credentials_provider() {
            self.aws_credentials = Some(Arc::new(AwsSdkCredentialProvider {
                provider: provider.clone(),
            }));
        }
    }

    /// Builds the store for `url`. Remote stores share this config's HTTP clients.
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

    /// Partition options and unrecognized keys
    fn partition_options_and_unrecognized(&mut self) {
        for (key, value) in self.unrecognized.clone() {
            let key_could_configure_object_store = AmazonS3ConfigKey::from_str(key.as_str())
                .is_ok()
                || object_store::gcp::GoogleConfigKey::from_str(key.as_str()).is_ok()
                || object_store::azure::AzureConfigKey::from_str(key.as_str()).is_ok()
                || object_store::client::ClientConfigKey::from_str(key.as_str()).is_ok();
            if key_could_configure_object_store {
                self.unrecognized
                    .remove(&key)
                    .expect("key should exist in the hashmap");
                // a hashmap cannot contain duplicate keys => ignore the replaced value
                let _ = match value {
                    serde_json::Value::Bool(b) => self.options.insert(key.clone(), b.to_string()),
                    serde_json::Value::Number(n) => self.options.insert(key.clone(), n.to_string()),
                    serde_json::Value::String(s) => self.options.insert(key.clone(), s.clone()),
                    v => {
                        // warn early with better context
                        warn!(
                            "Ignoring unrecognized configuration key 'pmtiles.{key}': {v:?}. Only boolean, string or number values are allowed here. Please check your configuration file for typos."
                        );
                        None
                    }
                };
            }
        }
    }

    /// Migrates old, deprecated keys to their new equivalents or warns about removed keys.
    fn migrate_deprecated_keys(&mut self) {
        if self.unrecognized.contains_key("dir_cache_size_mb") {
            warn!(
                "deprecated config: `pmtiles.dir_cache_size_mb` is no longer used. \
                 Use `cache.size_mb` in the root of the config file, \
                 or `pmtiles.directory_cache.size_mb` to override the PMTiles directory cache size"
            );
        }

        // below: http -> object_store
        if !self.options.contains_key("allow_http") {
            warn!(
                "Defaulting `pmtiles.allow_http` to `true`. This is likely to become an error in the future for better security."
            );
            self.options
                .insert("allow_http".to_owned(), true.to_string());
        }

        // below: AWS -> object_store
        // virtual_hosted_style_request is the exact opposite of force_path_style
        for key in ["aws_s3_force_path_style", "force_path_style"] {
            if let Some(Some(force_path_style)) = self.unrecognized.remove(key).map(|v| v.as_bool())
            {
                let virtual_hosted_style_request = !force_path_style;
                self.migrate_aws_value(
                    "Configuration option",
                    &format!("pmtiles.{key}"),
                    "virtual_hosted_style_request",
                    virtual_hosted_style_request.to_string(),
                );
            }
        }

        if let Ok(force_path_style) =
            env::var("AWS_S3_FORCE_PATH_STYLE").map(|v| v == "1" || v.to_lowercase() == "true")
        {
            let virtual_hosted_style_request = !force_path_style;
            self.migrate_aws_value(
                "Environment variable",
                "AWS_S3_FORCE_PATH_STYLE",
                "virtual_hosted_style_request",
                virtual_hosted_style_request.to_string(),
            );
        }

        // `AWS_NO_CREDENTIALS` was the name in some early documentation of this feature
        for key in ["aws_skip_credentials", "aws_no_credentials"] {
            if let Some(Some(no_credentials)) = self.unrecognized.remove(key).map(|v| v.as_bool()) {
                self.migrate_aws_value(
                    "Configuration option",
                    &format!("pmtiles.{key}"),
                    "skip_signature",
                    no_credentials.to_string(),
                );
            }
        }
        for env in ["AWS_SKIP_CREDENTIALS", "AWS_NO_CREDENTIALS"] {
            if let Ok(skip_credentials) =
                env::var(env).map(|v| v == "1" || v.to_lowercase() == "true")
            {
                self.migrate_aws_value(
                    "Environment variable",
                    env,
                    "skip_signature",
                    skip_credentials.to_string(),
                );
            }
        }

        // lowercase(env_key) => new key
        for env_key in [
            "AWS_ACCESS_KEY_ID",
            "AWS_SECRET_ACCESS_KEY",
            "AWS_SESSION_TOKEN",
            "AWS_REGION",
        ] {
            if let Ok(var) = env::var(env_key) {
                let new_key_with_aws_prefix = env_key.to_lowercase();
                let new_key_without_aws_prefix = new_key_with_aws_prefix
                    .strip_prefix("aws_")
                    .expect("all our keys start with aws_");
                self.migrate_aws_value(
                    "Environment variable",
                    env_key,
                    new_key_without_aws_prefix,
                    var,
                );
            }
        }
        if let Ok(profile) = env::var("AWS_PROFILE") {
            self.migrate_aws_profile("Environment variable", "AWS_PROFILE", profile);
        }
    }
    fn migrate_aws_profile(&mut self, r#type: &'static str, key: &str, value: String) {
        if self.profile.is_some() {
            warn!("{type} {key} is ignored in favor of the configuration value pmtiles.profile.");
        } else {
            warn!(
                "{type} {key} is deprecated. Please use pmtiles.profile in the configuration file instead."
            );
            self.profile = Some(value);
        }
    }

    fn migrate_aws_value(&mut self, r#type: &'static str, key: &str, new_key: &str, value: String) {
        let new_key_with_aws_prefix = format!("aws_{new_key}");
        if self.options.contains_key(new_key) {
            warn!(
                "{type} {key} is ignored in favor of the new configuration value pmtiles.{new_key}."
            );
        } else if self.options.contains_key(&new_key_with_aws_prefix) {
            warn!(
                "{type} {key} is ignored in favor of the new configuration value pmtiles.{new_key_with_aws_prefix}."
            );
        } else {
            warn!(
                "{type} {key} is deprecated. Please use pmtiles.{new_key} in the configuration file instead."
            );
            self.options.insert(new_key.to_owned(), value);
        }
    }

    /// Forwards the credential-discovery variables for [`AWS_CREDENTIAL_DISCOVERY_KEYS`] to the
    /// S3 client so task roles work without configuration.
    ///
    /// Without these, `object_store` falls back to the EC2 instance metadata service, which does
    /// not exist on ECS/Fargate or EKS. Explicitly configured keys win over the environment, and a
    /// `profile` is left alone because the AWS SDK chain it loads already covers these sources.
    fn import_aws_credential_discovery_env(&mut self, env: &impl Env) {
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
            info!("Using {env_key} from the environment as pmtiles.{bare} for S3 credentials.");
            self.options.insert(bare.to_owned(), value);
        }
    }
}

impl TileSourceConfiguration for PmtConfig {
    fn parse_urls() -> bool {
        true
    }

    async fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> SourceBuildResult<BoxedSource> {
        // canonicalize to resolve symlinks
        let path = path
            .canonicalize()
            .map_err(|e| ConfigFileError::IoError(e, path))?;
        // path->url conversion requires absolute path, otherwise it errors
        let path = std::path::absolute(&path).map_err(|e| ConfigFileError::IoError(e, path))?;
        // windows needs unix style paths, I.e. replace backslashes with forward slashes
        // a simple "add file://" does not work on windows
        // example: C:\Users\martin\Documents\pmtiles -> file://C:/Users/martin/Documents/pmtiles
        let url = Url::from_file_path(&path)
            .or(Err(ConfigFileError::PathNotConvertibleToUrl(path.clone())))?;
        trace!(
            "Pmtiles source {id} ({}) will be loaded as {url}",
            path.display()
        );
        self.new_sources_url(id, url, cache).await
    }

    async fn new_sources_url(
        &self,
        id: String,
        url: Url,
        cache: CachePolicy,
    ) -> SourceBuildResult<BoxedSource> {
        let (store, path) = self
            .parse_url_opts(&url)
            .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, id.clone()))?;
        let dir_cache = PmtCacheInstance::new_auto_id(self.pmtiles_directory_cache.clone());
        let source = PmtilesSource::new(dir_cache, id, store, path, cache.zoom()).await?;
        Ok(Box::new(source))
    }
}

/// One HTTP client per distinct [`ClientOptions`], handed to every store built from one
/// [`PmtConfig`], so its sources share connection pools instead of each opening their own.
#[derive(Clone, Debug, Default)]
pub(crate) struct SharedHttpClients(Arc<DashMap<String, HttpClient>>);

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

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use aws_runtime::env_config::file::{EnvConfigFileKind, EnvConfigFiles};
    use indoc::indoc;
    use rstest::rstest;
    use tempfile::tempdir;

    use super::*;
    use crate::config::primitives::env::FauxEnv;

    fn task_role_env() -> FauxEnv {
        [
            (
                "AWS_CONTAINER_CREDENTIALS_RELATIVE_URI",
                OsString::from("/v2/credentials/12345678-1234-1234-1234-123456789012"),
            ),
            (
                "AWS_WEB_IDENTITY_TOKEN_FILE",
                OsString::from("/var/run/secrets/eks.amazonaws.com/serviceaccount/token"),
            ),
            (
                "AWS_ROLE_ARN",
                OsString::from("arn:aws:iam::123456789012:role/from-env"),
            ),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn credential_discovery_env_reaches_the_s3_store() {
        let mut config = PmtConfig::default();
        config.import_aws_credential_discovery_env(&task_role_env());
        assert_eq!(
            config
                .options
                .get("container_credentials_relative_uri")
                .map(String::as_str),
            Some("/v2/credentials/12345678-1234-1234-1234-123456789012")
        );
        assert_eq!(
            config
                .options
                .get("web_identity_token_file")
                .map(String::as_str),
            Some("/var/run/secrets/eks.amazonaws.com/serviceaccount/token")
        );
        assert_eq!(
            config.options.get("role_arn").map(String::as_str),
            Some("arn:aws:iam::123456789012:role/from-env")
        );
        assert!(config.aws_credentials.is_none());
        // the forwarded keys must be ones object_store accepts
        config
            .parse_url_opts(&Url::parse("s3://bucket/tiles.pmtiles").unwrap())
            .unwrap();
    }

    #[test]
    fn explicit_configuration_wins_over_credential_discovery_env() {
        let mut config = PmtConfig::default();
        config.options.insert(
            "aws_role_arn".to_owned(),
            "arn:aws:iam::123456789012:role/explicit".to_owned(),
        );
        config.import_aws_credential_discovery_env(&task_role_env());
        assert!(!config.options.contains_key("role_arn"));
        assert_eq!(
            config.options.get("aws_role_arn").map(String::as_str),
            Some("arn:aws:iam::123456789012:role/explicit")
        );
        assert!(config.options.contains_key("web_identity_token_file"));
    }

    #[test]
    fn profile_disables_credential_discovery_env() {
        let mut config = PmtConfig {
            profile: Some("staging".to_owned()),
            ..PmtConfig::default()
        };
        config.import_aws_credential_discovery_env(&task_role_env());
        assert!(config.options.is_empty());
    }

    #[rstest]
    #[case::s3("s3://bucket-a/one.pmtiles", "s3://bucket-b/two.pmtiles")]
    #[case::https(
        "https://tiles.example.com/one.pmtiles",
        "https://other.example.com/two.pmtiles"
    )]
    #[case::gcs("gs://bucket-a/one.pmtiles", "gs://bucket-b/two.pmtiles")]
    #[case::azure("az://container-a/one.pmtiles", "az://container-b/two.pmtiles")]
    #[case::mixed("s3://bucket-a/one.pmtiles", "https://tiles.example.com/two.pmtiles")]
    fn remote_sources_share_http_clients(#[case] first: &str, #[case] second: &str) {
        let mut config = PmtConfig::default();
        config
            .options
            .insert("aws_region".to_owned(), "us-east-1".to_owned());
        config
            .options
            .insert("skip_signature".to_owned(), "true".to_owned());
        config.options.insert(
            "azure_storage_account_name".to_owned(),
            "account".to_owned(),
        );
        config.parse_url_opts(&Url::parse(first).unwrap()).unwrap();
        let clients = config.http_clients.0.len();
        assert!(clients > 0);
        config.parse_url_opts(&Url::parse(second).unwrap()).unwrap();
        assert_eq!(config.http_clients.0.len(), clients);
    }

    fn profile_files() -> (tempfile::TempDir, EnvConfigFiles) {
        let dir = tempdir().unwrap();
        let credentials_path = dir.path().join("credentials");
        let config_path = dir.path().join("config");
        std::fs::write(
            &credentials_path,
            indoc! {"
                [staging]
                aws_access_key_id = profile-key
                aws_secret_access_key = profile-secret
                aws_session_token = profile-token
            "},
        )
        .unwrap();
        std::fs::write(
            &config_path,
            indoc! {"
                [profile staging]
                region = eu-west-2
            "},
        )
        .unwrap();
        let files = EnvConfigFiles::builder()
            .with_file(EnvConfigFileKind::Credentials, credentials_path)
            .with_file(EnvConfigFileKind::Config, config_path)
            .build();
        (dir, files)
    }

    #[tokio::test]
    async fn profile_finalization_loads_credentials_and_preserves_explicit_options() {
        let (_dir, files) = profile_files();
        let mut profile: PmtConfig = serde_saphyr::from_str(indoc! {"
            aws_profile: staging
            region: eu-west-2
            skip_signature: false
        "})
        .unwrap();
        profile.aws_profile_files = Some(files.clone());
        profile.finalize().await.unwrap();
        assert_eq!(profile.profile.as_deref(), Some("staging"));
        assert_eq!(
            profile.options.get("region").map(String::as_str),
            Some("eu-west-2")
        );
        let credentials = profile
            .aws_credentials
            .as_ref()
            .expect("profile credentials should be configured")
            .get_credential()
            .await
            .unwrap();
        assert_eq!(credentials.key_id, "profile-key");
        assert_eq!(credentials.secret_key, "profile-secret");
        assert_eq!(credentials.token.as_deref(), Some("profile-token"));

        for (key, value) in [
            ("web_identity_token_file", "/tmp/token"),
            ("metadata_endpoint", "http://169.254.169.254"),
            ("aws_metadata_endpoint", "http://fd00:ec2::254"),
            ("imdsv1_fallback", "true"),
            ("aws_imdsv1_fallback", "true"),
            ("endpoint_url_sts", "http://localhost:4566"),
            ("aws_endpoint_url_sts", "http://localhost:4566"),
        ] {
            let mut explicit: PmtConfig = serde_saphyr::from_str(&format!(
                "profile: staging\nregion: us-east-2\n{key}: {value}\n"
            ))
            .unwrap();
            explicit.aws_profile_files = Some(files.clone());
            explicit.finalize().await.unwrap();
            assert_eq!(
                explicit.options.get("region").map(String::as_str),
                Some("us-east-2")
            );
            assert_eq!(explicit.options.get(key).map(String::as_str), Some(value));
            assert!(
                explicit.aws_credentials.is_none(),
                "{key} must retain object_store credential-provider precedence"
            );
        }
    }
}
