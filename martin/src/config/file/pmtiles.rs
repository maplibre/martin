use std::collections::HashMap;
use std::path::PathBuf;

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
    #[serde(flatten, default)]
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
        for key in [
            "aws_s3_force_path_style",
            "force_path_style",
            "aws_skip_credentials",
            "skip_credentials",
        ] {
            if self.unrecognized.contains_key(key) {
                warn!(
                    "{key} is no longer required. Please refer to https://docs.rs/object_store/latest/object_store/aws/struct.AmazonS3Builder.html for the available options."
                );
            }
        }
        for key in [
            "AWS_S3_FORCE_PATH_STYLE",
            "AWS_SKIP_CREDENTIALS",
            "AWS_NO_CREDENTIALS",
        ] {
            if std::env::var_os(key).is_some() {
                warn!(
                    "Environment variable {key} is no longer used. You must use the config file to configure the object store. Please refer to https://docs.rs/object_store/latest/object_store/aws/struct.AmazonS3Builder.html for the available options."
                );
            }
        }

        if self.unrecognized.contains_key("dir_cache_size_mb") {
            warn!(
                "dir_cache_size_mb is no longer used. Instead, use cache_size_mb param in the root of the config file."
            );
        }

        Ok(())
    }

    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
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
