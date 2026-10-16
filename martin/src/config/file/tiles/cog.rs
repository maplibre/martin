use std::collections::BTreeMap;
use std::fmt::Debug;
use std::mem;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt as _};
use martin_core::tiles::cog::{CogMosaicOptions, CogSource};
use martin_core::tiles::{BackendSource, BoxedSource};
use serde::ser::SerializeMap as _;
use serde::{Deserialize, Serialize, Serializer};
use tracing::info;
use url::Url;

use crate::config::file::process::ProcessConfig;
use crate::config::file::source_location::SourceLocation;
use crate::config::file::{
    CacheControlHeader, CachePolicy, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    ConfigurationLivecycleHooks, FileConfig, MAX_CONCURRENT_SOURCE_INITS, ObjectStoreConfig,
    ResolutionResult, SourceBuildResult, TileSourceConfiguration, TileSourceWarning,
    UnrecognizedValues, resolve_files,
};
#[cfg(feature = "processing")]
use crate::config::file::{ContourProcessConfig, HillshadeProcessConfig};
use crate::config::primitives::IdResolver;

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

    /// Sources that each serve many COG files as one, by source ID.
    #[serde(default)]
    pub mosaics: BTreeMap<String, CogMosaicSource>,

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

/// Default for [`CogMosaicSource::max_cached_files`].
pub const DEFAULT_MOSAIC_CACHED_FILES: u64 = 1000;

/// Default for [`CogMosaicSource::cached_files_ttl`].
pub const DEFAULT_MOSAIC_CACHED_FILES_TTL: Duration = Duration::from_mins(10);

/// A source that serves many COG files as one, each covering exactly one tile at `zoom`.
///
/// Files are opened on first use and kept in a bounded cache; tiles without a file are empty.
/// One file is found by listing the location at startup to learn the tile size and format.
#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CogMosaicSource {
    /// Where the files are, local or remote, naming each file by its tile at `zoom`:
    /// `{quadkey}`, or `{x}` and `{y}` with an optional `{z}`, e.g. `s3://bucket/ortho/{quadkey}.tif`
    /// or `/data/ortho/{z}/{x}/{y}.tif`. `{-y}` is the row counted from the bottom (TMS).
    pub path: String,
    /// The zoom level at which each file covers exactly one tile. It is also the lowest zoom served.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &16))]
    pub zoom: u8,
    /// The highest zoom served. Defaults to the highest zoom of the file opened at startup.
    pub maxzoom: Option<u8>,
    /// How many opened files (and known-missing ones) are kept at once. Defaults to 1000.
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &DEFAULT_MOSAIC_CACHED_FILES))]
    pub max_cached_files: Option<u64>,
    /// How long an opened file is kept before it is opened again, which picks up replaced and
    /// added files. Supports human-readable formats: "10m", "1h", "30s". Defaults to "10m".
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"10m")
    )]
    pub cached_files_ttl: Option<Duration>,
    /// Zoom-level bounds for tile caching.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CachePolicyShape")
    )]
    pub cache: CachePolicy,
    /// `Cache-Control` response header for this source.
    /// Overrides the top-level `cache_control` default.
    #[serde(default)]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub cache_control: Option<CacheControlHeader>,
    /// Hillshade settings for this source.
    ///
    /// Present means the source serves Mapzen *normal* tiles and Martin should bake a hillshade from them.
    /// See the hillshade documentation for the knobs.
    #[cfg(feature = "processing")]
    #[serde(default)]
    pub convert_to_hillshade: Option<HillshadeProcessConfig>,
    /// Trace contour lines from this source's tiles.
    ///
    /// Present means the source serves Mapzen *Terrarium* elevation tiles and Martin should trace contours from them.
    /// See the contour documentation for the knobs.
    #[cfg(feature = "processing")]
    #[serde(default)]
    pub convert_to_contour: Option<ContourProcessConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl CogMosaicSource {
    /// The per-source raster processing and `cache_control` settings.
    pub(crate) fn process(&self) -> ProcessConfig {
        ProcessConfig {
            convert_to_mlt: None,
            convert_to_mvt: None,
            cache_control: self.cache_control.clone(),
            #[cfg(feature = "processing")]
            convert_to_hillshade: self.convert_to_hillshade.clone(),
            #[cfg(feature = "processing")]
            convert_to_contour: self.convert_to_contour.clone(),
        }
    }

    fn options(&self) -> CogMosaicOptions {
        CogMosaicOptions {
            file_zoom: self.zoom,
            max_zoom: self.maxzoom,
            max_cached_files: self.max_cached_files.unwrap_or(DEFAULT_MOSAIC_CACHED_FILES),
            cached_files_ttl: self
                .cached_files_ttl
                .unwrap_or(DEFAULT_MOSAIC_CACHED_FILES_TTL),
        }
    }

    /// This mosaic with credentials, query and fragment removed from a remote `path`.
    fn sanitized(&self) -> Self {
        Self {
            path: sanitized_mosaic_path(&self.path),
            ..self.clone()
        }
    }
}

/// A mosaic path with credentials, query and fragment removed from its remote location.
fn sanitized_mosaic_path(path: &str) -> String {
    let (location, template) = split_mosaic_path(path);
    match SourceLocation::classify(&location) {
        Ok(SourceLocation::ObjectStore(url) | SourceLocation::Http(url)) => {
            format!("{}{template}", sanitized_url(&url))
        }
        Ok(SourceLocation::Local(_)) | Err(_) => path.to_owned(),
    }
}

/// Splits a mosaic path into the location of its files and the key template under it,
/// at the last `/` before the first placeholder. A URL query stays with the location.
fn split_mosaic_path(path: &str) -> (String, &str) {
    let (path, query) = path
        .split_once('?')
        .map_or((path, None), |(path, query)| (path, Some(query)));
    let before_placeholder = path.split('{').next().unwrap_or_default();
    let split = before_placeholder.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let (location, template) = path.split_at(split);
    let location = match query {
        Some(query) => format!("{location}?{query}"),
        None => location.to_owned(),
    };
    (location, template)
}

impl Default for CogConfig {
    fn default() -> Self {
        Self {
            recursive: None,
            cache: CachePolicy::default(),
            reload_interval: DEFAULT_RELOAD_INTERVAL,
            mosaics: BTreeMap::new(),
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
        if !self.mosaics.is_empty() {
            let mosaics: BTreeMap<_, _> = self
                .mosaics
                .iter()
                .map(|(id, mosaic)| (id, mosaic.sanitized()))
                .collect();
            map.serialize_entry("mosaics", &mosaics)?;
        }
        self.object_store.serialize_entries(&mut map)?;
        map.end()
    }
}

impl ConfigurationLivecycleHooks for CogConfig {
    async fn finalize(&mut self) -> ConfigFileResult<()> {
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
        self.cache.clone()
    }

    async fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> SourceBuildResult<BackendSource> {
        let cog = CogSource::new(id, path, cache.zoom()).await?;
        Ok(BackendSource::Cog(cog))
    }

    async fn new_sources_url(
        &self,
        id: String,
        url: Url,
        cache: CachePolicy,
    ) -> SourceBuildResult<BackendSource> {
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
        Ok(BackendSource::Cog(source))
    }
}

impl CogConfig {
    /// Opens every configured mosaic, at most [`MAX_CONCURRENT_SOURCE_INITS`] at once.
    /// A mosaic that fails to open is reported as a warning and stays in the config.
    async fn resolve_mosaics(
        &mut self,
        idr: &IdResolver,
        default_cache: CachePolicy,
    ) -> (Vec<BoxedSource>, Vec<TileSourceWarning>) {
        let planned: Vec<_> = mem::take(&mut self.mosaics)
            .into_iter()
            .map(|(id, mosaic)| {
                (
                    idr.resolve(&id, sanitized_mosaic_path(&mosaic.path)),
                    mosaic,
                )
            })
            .collect();
        let this = &*self;
        let default_cache = &default_cache;
        let opened = stream::iter(planned)
            .map(|(id, mosaic)| async move {
                let result = this
                    .new_mosaic_source(id.clone(), &mosaic, default_cache)
                    .await;
                (id, mosaic, result)
            })
            .buffered(MAX_CONCURRENT_SOURCE_INITS)
            .collect::<Vec<_>>()
            .await;

        let mut results = Vec::new();
        let mut warnings = Vec::new();
        for (id, mosaic, result) in opened {
            match result {
                Ok(src) => {
                    info!(
                        source.id = %id,
                        source.mosaic = %sanitized_mosaic_path(&mosaic.path),
                        "Configured mosaic source"
                    );
                    results.push(src.boxed_on(None));
                }
                Err(err) => warnings.push(TileSourceWarning::SourceError {
                    source_id: id.clone(),
                    error: err.to_string(),
                }),
            }
            self.mosaics.insert(id, mosaic);
        }
        (results, warnings)
    }

    /// Opens one mosaic, whose `path` names its files by their tile.
    async fn new_mosaic_source(
        &self,
        id: String,
        mosaic: &CogMosaicSource,
        default_cache: &CachePolicy,
    ) -> SourceBuildResult<BackendSource> {
        let (location, template) = split_mosaic_path(&mosaic.path);
        let options = mosaic.options();
        let cache = mosaic.cache.or(default_cache);
        let source = match SourceLocation::classify(&location)? {
            SourceLocation::ObjectStore(url) | SourceLocation::Http(url) => {
                let (store, prefix) = self
                    .object_store
                    .parse_url_opts(&url)
                    .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, id.clone()))?;
                CogSource::new_mosaic_object_store(
                    id,
                    Arc::from(store),
                    prefix,
                    template,
                    sanitized_mosaic_path(&mosaic.path),
                    options,
                    cache.zoom(),
                )
                .await?
            }
            SourceLocation::Local(root) => {
                let root = if root.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    root
                };
                CogSource::new_mosaic_dir(id, root, template, options, cache.zoom()).await?
            }
        };
        Ok(BackendSource::Cog(source))
    }
}

/// Resolves the COG sources and files, then the mosaics.
pub async fn resolve_cog(
    config: &mut FileConfig<CogConfig>,
    idr: &IdResolver,
    default_cache: CachePolicy,
) -> ResolutionResult {
    let (mut sources, mut warnings) =
        resolve_files(config, idr, &["tif", "tiff"], default_cache.clone(), None).await?;
    let default_cache = config.cache_or(&default_cache);
    let (mosaics, mosaic_warnings) = config.custom.resolve_mosaics(idr, default_cache).await;
    sources.extend(mosaics);
    warnings.extend(mosaic_warnings);
    Ok((sources, warnings))
}

fn sanitized_url(url: &Url) -> String {
    let mut url = url.clone();
    let _ = url.set_username("");
    let _ = url.set_password(None);
    url.set_query(None);
    url.set_fragment(None);
    url.to_string()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::split_mosaic_path;
    use crate::config::file::{CollectUnrecognizedKeys as _, Config};
    use crate::config::primitives::IdResolver;
    use crate::srv::RESERVED_KEYWORDS;

    #[rstest]
    #[case(
        "s3://bucket/ortho/{quadkey}.tif",
        "s3://bucket/ortho/",
        "{quadkey}.tif"
    )]
    #[case("s3://bucket/{z}/{x}/{y}.tif", "s3://bucket/", "{z}/{x}/{y}.tif")]
    #[case(
        "https://host/ortho/{quadkey}.tif?sig=abc",
        "https://host/ortho/?sig=abc",
        "{quadkey}.tif"
    )]
    #[case("/data/ortho/tile_{x}_{y}.tif", "/data/ortho/", "tile_{x}_{y}.tif")]
    #[case("{quadkey}.tif", "", "{quadkey}.tif")]
    #[case("/data/plain.tif", "/data/", "plain.tif")]
    fn splits_a_mosaic_path_at_its_first_placeholder(
        #[case] path: &str,
        #[case] location: &str,
        #[case] template: &str,
    ) {
        assert_eq!(split_mosaic_path(path), (location.to_owned(), template));
    }

    async fn resolve(yaml: &str) -> (Config, Vec<String>, serde_json::Value) {
        let mut config: Config = serde_saphyr::from_str(yaml).unwrap();
        config.finalize().await.unwrap();
        let state = config
            .resolve(&IdResolver::new(RESERVED_KEYWORDS))
            .await
            .unwrap();
        let mut ids = state.tile_manager.tile_sources().source_names();
        ids.sort();
        let saved = serde_saphyr::to_string(&config.with_catalog(&state.tile_manager)).unwrap();
        let saved: serde_json::Value = serde_saphyr::from_str(&saved).unwrap();
        (config, ids, saved["cog"].clone())
    }

    #[tokio::test]
    async fn a_mosaic_source_is_served_and_saved_as_configured() {
        let (_, ids, saved) = resolve(indoc::indoc! {"
            cog:
              mosaics:
                ortho:
                  path: ../tests/fixtures/cog/mosaic/{quadkey}.tif
                  zoom: 16
                  maxzoom: 18
                  max_cached_files: 10
                  cached_files_ttl: 1m
        "})
        .await;
        assert_eq!(ids, ["ortho"]);
        insta::assert_json_snapshot!(saved, @r#"
        {
          "mosaics": {
            "ortho": {
              "path": "../tests/fixtures/cog/mosaic/{quadkey}.tif",
              "zoom": 16,
              "maxzoom": 18,
              "max_cached_files": 10,
              "cached_files_ttl": "1m"
            }
          }
        }
        "#);
    }

    #[tokio::test]
    async fn an_invalid_mosaic_is_skipped_with_a_warning() {
        let (_, ids, _) = resolve(indoc::indoc! {"
            on_invalid: warn
            cog:
              mosaics:
                empty:
                  path: ../tests/fixtures/cog/mosaic/{quadkey}_missing.tif
                  zoom: 16
                bad_template:
                  path: ../tests/fixtures/cog/mosaic/{q}.tif
                  zoom: 16
                ortho:
                  path: ../tests/fixtures/cog/mosaic/{quadkey}.tif
                  zoom: 16
        "})
        .await;
        assert_eq!(ids, ["ortho"]);
    }

    #[tokio::test]
    async fn unrecognized_mosaic_keys_are_reported() {
        let mut config: Config = serde_saphyr::from_str(indoc::indoc! {"
            cog:
              mosaics:
                ortho:
                  path: ../tests/fixtures/cog/mosaic/{quadkey}.tif
                  zoom: 16
                  max_zoom: 18
        "})
        .unwrap();
        config.finalize().await.unwrap();
        insta::assert_debug_snapshot!(config.get_unrecognized_keys(), @r#"
        {
            "cog.mosaics.ortho.max_zoom",
        }
        "#);
    }
}
