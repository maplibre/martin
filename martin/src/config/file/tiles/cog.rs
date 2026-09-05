use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
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

#[derive(Clone, Debug, Deserialize, CollectUnrecognizedKeys)]
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

    /// How often configured remote objects (`s3://`, `https://`, …) are re-checked with a `HEAD`
    /// request for replacement. Local directories are watched via filesystem events and ignore
    /// this setting.
    ///
    /// Supports human-readable formats: "10m", "1h", "30s".
    /// Defaults to "10m". Set to "0s" to disable remote replacement detection.
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

    /// Versions captured by successful startup opens, shared across config clones.
    #[serde(skip)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    loaded_remote_versions:
        Arc<Mutex<BTreeMap<String, crate::config::file::tiles::discovery::Version>>>,

    /// Reload builds use the same config without mutating the startup-version snapshot.
    #[serde(skip, default = "record_loaded_remote_versions")]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    record_loaded_remote_versions: bool,
}

/// Default polling interval for
/// [`CogReloader`](crate::config::file::reload::cog::CogReloader) to re-check configured remote
/// objects for replacement. Local directories are notify-driven and ignore this setting.
pub const DEFAULT_RELOAD_INTERVAL: Duration = Duration::from_mins(10);

fn default_reload_interval() -> Duration {
    DEFAULT_RELOAD_INTERVAL
}

const fn record_loaded_remote_versions() -> bool {
    true
}

impl Default for CogConfig {
    fn default() -> Self {
        Self {
            recursive: None,
            cache: CachePolicy::default(),
            reload_interval: DEFAULT_RELOAD_INTERVAL,
            object_store: ObjectStoreConfig::default(),
            unrecognized: UnrecognizedValues::default(),
            loaded_remote_versions: Arc::default(),
            record_loaded_remote_versions: true,
        }
    }
}

impl PartialEq for CogConfig {
    fn eq(&self, other: &Self) -> bool {
        self.recursive == other.recursive
            && self.cache == other.cache
            && self.reload_interval == other.reload_interval
            && self.object_store == other.object_store
            && self.unrecognized == other.unrecognized
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

impl CogConfig {
    pub(crate) fn loaded_remote_versions(
        &self,
    ) -> BTreeMap<String, crate::config::file::tiles::discovery::Version> {
        self.loaded_remote_versions
            .lock()
            .expect("loaded COG version map mutex")
            .clone()
    }

    pub(crate) fn for_reload(mut self) -> Self {
        self.record_loaded_remote_versions = false;
        self
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
            id.clone(),
            Arc::from(store),
            path,
            sanitized_url(&url),
            cache.zoom(),
        )
        .await?;
        if self.record_loaded_remote_versions {
            let version = crate::config::file::tiles::discovery::version_from_cog_meta(
                source.object_metadata(),
            );
            self.loaded_remote_versions
                .lock()
                .expect("loaded COG version map mutex")
                .insert(id, version);
        }
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
