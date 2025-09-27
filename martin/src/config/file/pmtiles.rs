use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;

use log::warn;
use martin_core::cache::OptMainCache;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::pmtiles::{PmtCache, PmtilesSource};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{
    ConfigExtras, ConfigFileError, ConfigFileResult, SourceConfigExtras, UnrecognizedKeys,
    UnrecognizedValues,
};

#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PmtConfig {
    // if the key is the allowed set, we assume it is there for a purpose
    #[serde(skip)]
    pub options: HashMap<String, String>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,

    /// Internal state => not serialized
    #[serde(skip)]
    cache: OptMainCache,
}

impl PartialEq for PmtConfig {
    fn eq(&self, other: &Self) -> bool {
        self.options == other.options && self.unrecognized == other.unrecognized
    }
}

impl ConfigExtras for PmtConfig {
    fn init_parsing(&mut self, cache: OptMainCache) -> ConfigFileResult<()> {
        assert!(
            self.cache.is_none(),
            "init_parsing should only be called once"
        );
        self.cache = cache;

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
                    serde_yaml::Value::String(s) => self.options.insert(key.clone(), s.to_string()),
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

        // below: AWS -> object_store
        for key in ["aws_s3_force_path_style", "force_path_style"] {
            if self.unrecognized.contains_key(key) {
                warn!(
                    "{key} is no longer used as path style urls are natively supported without additional configuration"
                );
            }
        }

        if std::env::var_os("AWS_S3_FORCE_PATH_STYLE").is_some() {
            warn!(
                "Environment variable AWS_S3_FORCE_PATH_STYLE is no longer used as path style urls are natively supported without additional configuration"
            );
        }

        // `AWS_NO_CREDENTIALS` was the name in some early documentation of this feature
        for key in ["aws_skip_credentials", "aws_no_credentials"] {
            if let Some(Some(no_credentials)) = self.unrecognized.remove(key).map(|v| v.as_bool()) {
                warn!(
                    "Configuration option pmtiles.{key} is deprecated. Please use pmtiles.skip_signature instead."
                );
                self.options
                    .insert("skip_signature".to_string(), no_credentials.to_string());
            }
        }
        for env in ["AWS_SKIP_CREDENTIALS", "AWS_NO_CREDENTIALS"] {
            if let Ok(Ok(no_credentials)) = std::env::var(env).map(|v| v.parse::<bool>()) {
                warn!(
                    "Environment variable {env} is deprecated. Please use pmtiles.skip_signature instead."
                );
                self.options
                    .insert("skip_signature".to_string(), no_credentials.to_string());
            }
        }
    }
}

impl SourceConfigExtras for PmtConfig {
    fn parse_urls() -> bool {
        true
    }

    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<BoxedSource> {
        // canonicalize to get rid of symlinks
        let path = path
            .canonicalize()
            .map_err(|e| ConfigFileError::IoError(e, path))?;
        // object_store does not support relative paths
        let path = std::path::absolute(&path).map_err(|e| ConfigFileError::IoError(e, path))?;
        let url = format!("file://{}", path.display());
        let url = url
            .parse()
            .map_err(|e| ConfigFileError::InvalidSourceUrl(e, url))?;
        self.new_sources_url(id, url).await
    }

    async fn new_sources_url(&self, id: String, url: Url) -> MartinResult<BoxedSource> {
        let (store, path) = object_store::parse_url_opts(&url, &self.options)
            .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, id.clone()))?;
        let cache = PmtCache::from(self.cache.clone());
        let source = PmtilesSource::new(cache, id, store, path).await?;
        Ok(Box::new(source))
    }
}
