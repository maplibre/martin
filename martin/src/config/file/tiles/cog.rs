use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::cog::CogSource;
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Serialize, Serializer};
use url::Url;

use crate::config::file::{
    CachePolicy, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    ConfigurationLivecycleHooks, ObjectStoreConfig, SourceBuildResult, TileSourceConfiguration,
    UnrecognizedValues,
};

#[derive(Clone, Debug, Deserialize, PartialEq, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CogConfig {
    /// Whether `paths` are scanned recursively
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &false))]
    pub recursive: Option<bool>,
    /// Zoom-level bounds for caching the tiles of every COG source without its own `cache`.
    /// Overrides the top-level `cache` bounds.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,

    /// How often configured remote objects (`s3://`, `https://`, …) are re-checked for
    /// replacement and remote prefixes are re-listed for additions, replacements, and removals.
    /// Local directories are watched via filesystem events and ignore this setting.
    ///
    /// Supports human-readable formats: "10m", "1h", "30s".
    /// Defaults to "10m". Set to "0s" to disable remote polling and prefix discovery.
    #[serde(default = "default_reload_interval", with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "String", example = &"10m")
    )]
    pub reload_interval: Duration,

    /// Authentication, endpoint, and HTTP client settings for remote COGs.
    #[serde(flatten)]
    pub object_store: ObjectStoreConfig,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

/// Default polling interval for
/// [`CogReloader`](crate::config::file::tiles::reload::cog::CogReloader) to check configured remote
/// objects and list remote prefixes. Local directories are notify-driven and ignore this setting.
pub const DEFAULT_RELOAD_INTERVAL: Duration = Duration::from_mins(10);

fn default_reload_interval() -> Duration {
    DEFAULT_RELOAD_INTERVAL
}

impl Default for CogConfig {
    fn default() -> Self {
        Self {
            recursive: None,
            cache: CachePolicy::default(),
            reload_interval: DEFAULT_RELOAD_INTERVAL,
            object_store: ObjectStoreConfig::default(),
            unrecognized: UnrecognizedValues::default(),
        }
    }
}

impl Serialize for CogConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        if let Some(recursive) = self.recursive {
            map.serialize_entry("recursive", &recursive)?;
        }
        if !self.cache.is_empty() {
            map.serialize_entry("cache", &self.cache)?;
        }
        if self.reload_interval != DEFAULT_RELOAD_INTERVAL {
            map.serialize_entry(
                "reload_interval",
                &humantime_serde::Serde::from(&self.reload_interval),
            )?;
        }
        self.object_store.serialize_entries(&mut map)?;
        map.end()
    }
}

impl ConfigurationLivecycleHooks for CogConfig {
    async fn finalize(&mut self) -> ConfigFileResult<()> {
        // Match the long-standing PMTiles URL behavior without warning for local-only COG
        // configurations. An explicit value still wins during option partitioning below.
        self.object_store
            .options
            .entry("allow_http".to_owned())
            .or_insert_with(|| "true".to_owned());
        self.object_store.prepare(&mut self.unrecognized, "cog");
        self.object_store.finalize_runtime("cog").await;
        Ok(())
    }
}

impl TileSourceConfiguration for CogConfig {
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
        let cog = CogSource::new(id, path, cache.zoom()).await?;
        Ok(Box::new(cog))
    }

    async fn new_sources_url(
        &self,
        id: String,
        url: Url,
        cache: CachePolicy,
    ) -> SourceBuildResult<BoxedSource> {
        let (store, path) = self
            .object_store
            .parse_url_opts(&url)
            .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, id.clone()))?;
        let source = CogSource::new_object_store(
            id,
            Arc::from(store),
            path,
            sanitized_url(&url),
            cache.zoom(),
        )
        .await?;
        Ok(Box::new(source))
    }
}

fn sanitized_url(url: &Url) -> String {
    let mut url = url.clone();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}
