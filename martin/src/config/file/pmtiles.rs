use std::path::PathBuf;

use log::warn;
use martin_core::cache::OptMainCache;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::pmtiles::{PmtCache, PmtFileSource, PmtHttpSource, PmtS3Source};
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
    /// Force path style URLs for S3 buckets
    ///
    /// A path style URL is a URL that uses the bucket name as part of the path like `example.org/some_bucket` instead of the hostname `some_bucket.example.org`.
    /// If `None` (the default), this will look at `AWS_S3_FORCE_PATH_STYLE` or default to `false`.
    #[serde(default, alias = "aws_s3_force_path_style")]
    pub force_path_style: Option<bool>,
    /// Skip loading credentials for S3 buckets
    ///
    /// Set this to `true` to request anonymously for publicly available buckets.
    /// If `None` (the default), this will look at `AWS_SKIP_CREDENTIALS` and `AWS_NO_CREDENTIALS` or default to `false`.
    #[serde(default, alias = "aws_skip_credentials")]
    pub skip_credentials: Option<bool>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,

    /// Internal state => not serialized
    #[serde(skip)]
    cache: OptMainCache,
}

impl PartialEq for PmtConfig {
    fn eq(&self, other: &Self) -> bool {
        self.force_path_style == other.force_path_style
            && self.skip_credentials == other.skip_credentials
            && self.unrecognized == other.unrecognized
    }
}

impl ConfigExtras for PmtConfig {
    fn init_parsing(&mut self, cache: OptMainCache) -> ConfigFileResult<()> {
        if self.cache.is_some() {
            return Err(ConfigFileError::InitParsingCalledTwice);
        }
        self.cache = cache;

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
        Ok(Box::new(
            PmtFileSource::new(PmtCache::from(self.cache.clone()), id, path).await?,
        ))
    }

    async fn new_sources_url(&self, id: String, url: Url) -> MartinResult<BoxedSource> {
        match url.scheme() {
            "s3" => {
                let force_path_style = self.force_path_style.unwrap_or_else(|| {
                    get_env_as_bool("AWS_S3_FORCE_PATH_STYLE").unwrap_or_default()
                });
                let skip_credentials = self.skip_credentials.unwrap_or_else(|| {
                    get_env_as_bool("AWS_SKIP_CREDENTIALS").unwrap_or_else(|| {
                        // `AWS_NO_CREDENTIALS` was the name in some early documentation of this feature
                        get_env_as_bool("AWS_NO_CREDENTIALS").unwrap_or_default()
                    })
                });
                Ok(Box::new(
                    PmtS3Source::new(
                        PmtCache::from(self.cache.clone()),
                        id,
                        url,
                        skip_credentials,
                        force_path_style,
                    )
                    .await?,
                ))
            }
            _ => Ok(Box::new(
                PmtHttpSource::new(PmtCache::from(self.cache.clone()), id, url).await?,
            )),
        }
    }
}

/// Interpret an environment variable as a [`bool`]
///
/// This ignores casing and treats bad utf8 encoding as `false`.
fn get_env_as_bool(key: &'static str) -> Option<bool> {
    let val = std::env::var_os(key)?.to_ascii_lowercase();
    Some(val.to_str().is_some_and(|val| val == "1" || val == "true"))
}
