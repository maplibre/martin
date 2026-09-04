use std::env;
use std::path::PathBuf;
use std::time::Duration;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesSource};
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Serialize, Serializer};
use tracing::{trace, warn};
use url::Url;

use crate::config::file::{
    CachePolicy, CacheSizeConfig, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    ConfigurationLivecycleHooks, ObjectStoreConfig, SourceBuildResult, TileSourceConfiguration,
    UnrecognizedValues,
};
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};

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
#[derive(Debug, Clone, Deserialize, CollectUnrecognizedKeys)]
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

    /// Shared remote object-store settings.
    #[serde(flatten)]
    pub object_store: ObjectStoreConfig,

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

    /// Whether `paths` are scanned recursively
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &false))]
    pub recursive: Option<bool>,
    /// Zoom-level bounds for caching the tiles of every `PMTiles` source without its own `cache`.
    /// Overrides the top-level `cache` bounds.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,

    /// `PMTiles` directory cache (internal state, not serialized)
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub pmtiles_directory_cache: PmtCache,
}

impl Default for PmtConfig {
    fn default() -> Self {
        Self {
            directory_cache: CacheSizeConfig::default(),
            reload_interval: DEFAULT_RELOAD_INTERVAL,
            object_store: ObjectStoreConfig::default(),
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mlt: None,
            #[cfg(all(feature = "mlt", feature = "_tiles"))]
            convert_to_mvt: None,
            recursive: None,
            cache: CachePolicy::default(),
            unrecognized: UnrecognizedValues::default(),
            pmtiles_directory_cache: PmtCache::default(),
        }
    }
}

impl Serialize for PmtConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        if !self.directory_cache.is_empty() {
            map.serialize_entry("directory_cache", &self.directory_cache)?;
        }
        if !is_default_reload_interval(&self.reload_interval) {
            map.serialize_entry(
                "reload_interval",
                &humantime_serde::Serde::from(&self.reload_interval),
            )?;
        }
        self.object_store.serialize_entries(&mut map)?;
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        if let Some(config) = &self.convert_to_mlt {
            map.serialize_entry("convert_to_mlt", config)?;
        }
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        if let Some(config) = &self.convert_to_mvt {
            map.serialize_entry("convert_to_mvt", config)?;
        }
        if let Some(recursive) = self.recursive {
            map.serialize_entry("recursive", &recursive)?;
        }
        if !self.cache.is_empty() {
            map.serialize_entry("cache", &self.cache)?;
        }
        map.end()
    }
}

impl PartialEq for PmtConfig {
    fn eq(&self, other: &Self) -> bool {
        let base = self.directory_cache == other.directory_cache
            && self.reload_interval == other.reload_interval
            && self.object_store == other.object_store
            && self.recursive == other.recursive
            && self.cache == other.cache
            && self.unrecognized == other.unrecognized;
        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        let base = base
            && self.convert_to_mlt == other.convert_to_mlt
            && self.convert_to_mvt == other.convert_to_mvt;
        // pmtiles_directory_cache and http_clients are intentionally excluded from equality check
        base
    }
}

impl std::ops::Deref for PmtConfig {
    type Target = ObjectStoreConfig;

    fn deref(&self) -> &Self::Target {
        &self.object_store
    }
}

impl std::ops::DerefMut for PmtConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.object_store
    }
}

impl ConfigurationLivecycleHooks for PmtConfig {
    async fn finalize(&mut self) -> ConfigFileResult<()> {
        self.object_store.prepare(&mut self.unrecognized, "pmtiles");
        self.migrate_pmtiles_legacy_env();
        self.object_store.finalize_runtime("pmtiles").await;
        Ok(())
    }
}

impl PmtConfig {
    /// Retains PMTiles-only environment migrations while object-store behavior lives in
    /// [`ObjectStoreConfig`].
    fn migrate_pmtiles_legacy_env(&mut self) {
        if self.unrecognized.contains_key("dir_cache_size_mb") {
            warn!(
                "deprecated config: `pmtiles.dir_cache_size_mb` is no longer used. \
                 Use `cache.size_mb` in the root of the config file, \
                 or `pmtiles.directory_cache.size_mb` to override the PMTiles directory cache size"
            );
        }

        if let Ok(force_path_style) =
            env::var("AWS_S3_FORCE_PATH_STYLE").map(|v| v == "1" || v.to_lowercase() == "true")
        {
            let virtual_hosted_style_request = !force_path_style;
            self.object_store.migrate_aws_value(
                "Environment variable",
                "AWS_S3_FORCE_PATH_STYLE",
                "virtual_hosted_style_request",
                virtual_hosted_style_request.to_string(),
                "pmtiles",
            );
        }

        // `AWS_NO_CREDENTIALS` was the name in early PMTiles documentation.
        for env in ["AWS_SKIP_CREDENTIALS", "AWS_NO_CREDENTIALS"] {
            if let Ok(skip_credentials) =
                env::var(env).map(|v| v == "1" || v.to_lowercase() == "true")
            {
                self.object_store.migrate_aws_value(
                    "Environment variable",
                    env,
                    "skip_signature",
                    skip_credentials.to_string(),
                    "pmtiles",
                );
            }
        }

        if let Ok(profile) = env::var("AWS_PROFILE") {
            if self.profile.is_some() {
                warn!(
                    "Environment variable AWS_PROFILE is ignored in favor of the configuration value pmtiles.profile."
                );
            } else {
                warn!(
                    "Environment variable AWS_PROFILE is deprecated. Please use pmtiles.profile in the configuration file instead."
                );
                self.profile = Some(profile);
            }
        }
    }
}
impl TileSourceConfiguration for PmtConfig {
    fn parse_urls() -> bool {
        true
    }

    fn cache(&self) -> CachePolicy {
        self.cache
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
        config.import_aws_credential_discovery_env(&task_role_env(), "pmtiles");
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
        config.import_aws_credential_discovery_env(&task_role_env(), "pmtiles");
        assert!(!config.options.contains_key("role_arn"));
        assert_eq!(
            config.options.get("aws_role_arn").map(String::as_str),
            Some("arn:aws:iam::123456789012:role/explicit")
        );
        assert!(config.options.contains_key("web_identity_token_file"));
    }

    #[test]
    fn profile_disables_credential_discovery_env() {
        let mut config = PmtConfig::default();
        config.object_store.profile = Some("staging".to_owned());
        config.import_aws_credential_discovery_env(&task_role_env(), "pmtiles");
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
