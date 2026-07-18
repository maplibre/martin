use std::collections::HashMap;
use std::env;
use std::path::PathBuf;
use std::str::FromStr as _;
use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
#[allow(deprecated)]
use aws_config::profile::ProfileFileRegionProvider;
#[cfg(test)]
#[allow(deprecated)]
use aws_config::profile::profile_file::ProfileFiles;
use aws_credential_types::provider::{ProvideCredentials as _, SharedCredentialsProvider};
use martin_core::tiles::BoxedSource;
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesSource};
use object_store::aws::{AmazonS3Builder, AwsCredential, AwsCredentialProvider};
use object_store::{CredentialProvider, ObjectStore, ObjectStoreScheme};
use serde::{Deserialize, Serialize};
use tracing::{trace, warn};
use url::Url;

use crate::MartinResult;
use crate::config::file::{
    CachePolicy, CacheSizeConfig, ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks,
    TileSourceConfiguration, UnrecognizedKeys, UnrecognizedValues,
};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::primitives::AutoOption;

/// Default polling interval for [`PmtilesReloader`](crate::config::file::reload::pmtiles::PmtilesReloader)
/// to re-list remote URL prefixes (s3://, gs://, https://, etc.). Local directories are
/// notify-driven and ignore this setting.
pub const DEFAULT_RELOAD_INTERVAL: Duration = Duration::from_mins(10);

fn default_reload_interval() -> Duration {
    DEFAULT_RELOAD_INTERVAL
}

fn is_default_reload_interval(v: &Duration) -> bool {
    *v == DEFAULT_RELOAD_INTERVAL
}

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Serialize, Deserialize)]
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

    #[cfg(test)]
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    #[allow(deprecated)]
    pub(crate) aws_profile_files: Option<ProfileFiles>,
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
        // pmtiles_directory_cache is intentionally excluded from equality check
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
        self.load_aws_profile().await;

        Ok(())
    }

    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        #[cfg_attr(not(all(feature = "mlt", feature = "_tiles")), allow(unused_mut))]
        let mut keys: UnrecognizedKeys = self.unrecognized.keys().cloned().collect();
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        {
            if let Some(AutoOption::Explicit(cfg)) = self.convert_to_mlt.as_ref() {
                keys.extend(
                    cfg.unrecognized_keys()
                        .map(|k| format!("convert_to_mlt.{k}")),
                );
            }
            if let Some(AutoOption::Explicit(cfg)) = self.convert_to_mvt.as_ref() {
                keys.extend(
                    cfg.unrecognized_keys()
                        .map(|k| format!("convert_to_mvt.{k}")),
                );
            }
        }
        keys
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
        if ![
            "region",
            "aws_region",
            "default_region",
            "aws_default_region",
        ]
        .iter()
        .any(|key| self.options.contains_key(*key))
            && let Some(region) = sdk_config.region()
        {
            self.options
                .insert("region".to_string(), region.as_ref().to_string());
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
        ]
        .iter()
        .any(|key| self.options.contains_key(*key));
        let skips_signature = ["skip_signature", "aws_skip_signature"].iter().any(|key| {
            self.options
                .get(*key)
                .is_some_and(|value| value.eq_ignore_ascii_case("true") || value == "1")
        });

        if !has_explicit_credentials
            && !skips_signature
            && let Some(provider) = sdk_config.credentials_provider()
        {
            self.aws_credentials = Some(Arc::new(AwsSdkCredentialProvider {
                provider: provider.clone(),
            }));
        }
    }

    pub(crate) fn parse_url_opts(
        &self,
        url: &Url,
    ) -> object_store::Result<(Box<dyn ObjectStore>, object_store::path::Path)> {
        let (scheme, path) = ObjectStoreScheme::parse(url)?;
        if scheme != ObjectStoreScheme::AmazonS3 {
            return object_store::parse_url_opts(url, &self.options);
        }

        let mut builder = self.options.iter().fold(
            AmazonS3Builder::new().with_url(url.to_string()),
            |builder, (key, value)| match key.parse() {
                Ok(key) => builder.with_config(key, value),
                Err(_) => builder,
            },
        );
        if let Some(credentials) = &self.aws_credentials {
            builder = builder.with_credentials(credentials.clone());
        }
        Ok((Box::new(builder.build()?), path))
    }

    /// Partition options and unrecognized keys
    fn partition_options_and_unrecognized(&mut self) {
        for (key, value) in self.unrecognized.clone() {
            let key_could_configure_object_store =
                object_store::aws::AmazonS3ConfigKey::from_str(key.as_str()).is_ok()
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
                .insert("allow_http".to_string(), true.to_string());
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
            self.options.insert(new_key.to_string(), value);
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
    ) -> MartinResult<BoxedSource> {
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
    ) -> MartinResult<BoxedSource> {
        let (store, path) = self
            .parse_url_opts(&url)
            .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, id.clone()))?;
        let dir_cache = PmtCacheInstance::new_auto_id(self.pmtiles_directory_cache.clone());
        let source = PmtilesSource::new(dir_cache, id, store, path, cache.zoom()).await?;
        Ok(Box::new(source))
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
            key_id: credentials.access_key_id().to_string(),
            secret_key: credentials.secret_access_key().to_string(),
            token: credentials.session_token().map(ToString::to_string),
        }))
    }
}

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use aws_config::profile::profile_file::{ProfileFileKind, ProfileFiles};
    use indoc::indoc;
    use tempfile::tempdir;

    use super::*;

    fn profile_files() -> (tempfile::TempDir, ProfileFiles) {
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
        let files = ProfileFiles::builder()
            .with_file(ProfileFileKind::Credentials, credentials_path)
            .with_file(ProfileFileKind::Config, config_path)
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

        let mut explicit: PmtConfig = serde_saphyr::from_str(indoc! {"
            profile: staging
            region: us-east-2
            web_identity_token_file: /tmp/token
            role_arn: arn:aws:iam::123456789012:role/test
        "})
        .unwrap();
        explicit.aws_profile_files = Some(files);
        explicit.finalize().await.unwrap();
        assert_eq!(
            explicit.options.get("region").map(String::as_str),
            Some("us-east-2")
        );
        assert!(explicit.aws_credentials.is_none());
    }
}
