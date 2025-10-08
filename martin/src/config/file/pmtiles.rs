use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use log::{trace, warn};
use martin_core::tiles::BoxedSource;
use martin_core::tiles::pmtiles::{PmtCache, PmtCacheInstance, PmtilesSource};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, TileSourceConfiguration,
    UnrecognizedKeys, UnrecognizedValues,
};

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PmtConfig {
    /// Size of the directory cache in megabytes (0 to disable)
    ///
    /// Overrides [`cache_size_mb`](crate::config::file::Config::cache_size_mb).
    pub directory_cache_size_mb: Option<u64>,

    // if the key is the allowed set, we assume it is there for a purpose
    // settings and unreconginsed values are partitioned from each other in the init_parsing step
    #[serde(skip)]
    pub options: HashMap<String, String>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,

    /// PMTiles directory cache (internal state, not serialized)
    #[serde(skip)]
    pub pmtiles_directory_cache: PmtCache,
}

impl PartialEq for PmtConfig {
    fn eq(&self, other: &Self) -> bool {
        self.options == other.options && self.unrecognized == other.unrecognized
        // pmtiles_directory_cache is intentionally excluded from equality check
    }
}

impl ConfigurationLivecycleHooks for PmtConfig {
    fn finalize(&mut self) -> ConfigFileResult<()> {
        // if the key is the allowed set, we assume it is there for a purpose
        // because of how serde(flatten) works, we need to collect all in one place and then
        // partition them into options and unrecognized keys
        //
        // If we don't do this, the error message is not clear enough
        self.partition_options_and_unrecognized();
        self.migrate_deprecated_keys();

        Ok(())
    }

    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl PmtConfig {
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
                    serde_yaml::Value::Bool(b) => self.options.insert(key.clone(), b.to_string()),
                    serde_yaml::Value::Number(n) => self.options.insert(key.clone(), n.to_string()),
                    serde_yaml::Value::String(s) => self.options.insert(key.clone(), s.clone()),
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
                "dir_cache_size_mb is no longer used. Instead, use cache_size_mb param in the root of the config file."
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
            std::env::var("AWS_S3_FORCE_PATH_STYLE").map(|v| v == "1" || v.to_lowercase() == "true")
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
                std::env::var(env).map(|v| v == "1" || v.to_lowercase() == "true")
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
            if let Ok(var) = std::env::var(env_key) {
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
        if std::env::var("AWS_PROFILE").is_ok() {
            warn!(
                "Environment variable AWS_PROFILE not supported anymore. Supporting this is in scope, but would need more work. See https://github.com/pola-rs/polars/issues/18757#issuecomment-2379398284"
            );
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

    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<BoxedSource> {
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
        self.new_sources_url(id, url).await
    }

    async fn new_sources_url(&self, id: String, url: Url) -> MartinResult<BoxedSource> {
        use std::sync::LazyLock;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static NEXT_CACHE_ID: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(0));
        let cache_id = NEXT_CACHE_ID.fetch_add(1, Ordering::SeqCst);

        let (store, path) = object_store::parse_url_opts(&url, &self.options)
            .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, id.clone()))?;
        let cache = PmtCacheInstance::new(cache_id, self.pmtiles_directory_cache.clone());
        let source = PmtilesSource::new(cache, id, store, path).await?;
        Ok(Box::new(source))
    }
}
