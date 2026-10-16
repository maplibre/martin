#[cfg(feature = "_tiles")]
use std::collections::HashMap;
use std::collections::{BTreeMap, HashSet};
use std::fmt::{self, Debug};
#[cfg(feature = "_tiles")]
use std::mem;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(feature = "_tiles")]
use futures::stream::{self, StreamExt as _};
pub use martin_config_macros::ConfigurationLivecycleHooks;
use martin_core::CacheZoomRange;
#[cfg(feature = "_tiles")]
use martin_core::tiles::BackendSource;
#[cfg(feature = "_tiles")]
use martin_tile_utils::TileGrid;
use serde::de::value::MapAccessDeserializer;
use serde::de::{self, MapAccess, Visitor};
use serde::ser::{Error as _, SerializeMap as _};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
#[cfg(feature = "_tiles")]
use tracing::{info, warn};
#[cfg(feature = "_tiles")]
use url::Url;

#[cfg(all(feature = "processing", feature = "_tiles"))]
use crate::config::file::ContourProcessConfig;
#[cfg(all(feature = "processing", feature = "_tiles"))]
use crate::config::file::HillshadeProcessConfig;
#[cfg(feature = "_tiles")]
use crate::config::file::TileGrids;
#[cfg(feature = "_tiles")]
use crate::config::file::source_location::SourceLocation;
use crate::config::file::{
    CacheControlHeader, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    UnrecognizedValues,
};
#[cfg(feature = "_tiles")]
use crate::config::file::{MltProcessConfig, MvtProcessConfig};
#[cfg(feature = "_tiles")]
use crate::config::file::{ResolutionResult, TileSourceWarning};
#[cfg(feature = "_tiles")]
use crate::config::file::{SourceBuildError, SourceBuildResult};
#[cfg(feature = "_tiles")]
use crate::config::primitives::IdResolver;
use crate::config::primitives::one_or_many;

/// Lifecycle hooks for configuring the application
///
/// The hooks are guaranteed called in the following order:
/// 1. `finalize`
/// 2. [`CollectUnrecognizedKeys::get_unrecognized_keys`]
pub trait ConfigurationLivecycleHooks:
    CollectUnrecognizedKeys + Clone + Debug + Default + PartialEq + Send
{
    /// Finalize configuration discovery and patch old values
    ///
    /// In practice, this method is only implemented on a path of the config if a value or a value in the path below it needs to be finalized
    fn finalize(&mut self) -> impl Future<Output = ConfigFileResult<()>> + Send {
        async { Ok(()) }
    }
}

/// Configuration which all of our tile sources implement to make configuring them easier
#[cfg(feature = "_tiles")]
pub trait TileSourceConfiguration: ConfigurationLivecycleHooks {
    /// Indicates whether path strings for this configuration should be parsed as URLs.
    ///
    /// - `true` means any source path starting with `http://`, `https://`, or `s3://` will be treated as a remote URL.
    /// - `false` means all paths are treated as local file system paths.
    #[must_use]
    fn parse_urls() -> bool;

    /// The kind level cache bounds, for every source of this kind without its own.
    #[must_use]
    fn cache(&self) -> CachePolicy;

    /// Asynchronously creates a new `BackendSource` from a **local** file `path` using the given `id`.
    ///
    /// This function is called for each discovered file path that is not a URL.
    /// `cache` contains per-source zoom bounds, already merged with defaults.
    fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> impl Future<Output = SourceBuildResult<BackendSource>> + Send;

    /// Asynchronously creates a new `BackendSource` from a **remote** `url` using the given `id`.
    ///
    /// This function is called for each discovered source path that is a valid URL.
    /// `cache` contains per-source zoom bounds, already merged with defaults.
    fn new_sources_url(
        &self,
        id: String,
        url: Url,
        cache: CachePolicy,
    ) -> impl Future<Output = SourceBuildResult<BackendSource>> + Send;
}

/// The accepted shapes of a [`FileConfig`] field deserialized via [`path_or_config::deserialize`].
pub mod path_or_config {
    use std::fmt;
    use std::marker::PhantomData;
    use std::path::PathBuf;

    use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
    use serde::de::{self, MapAccess, SeqAccess, Visitor};
    use serde::{Deserialize, Deserializer};

    use super::FileConfig;

    /// A path, a list of paths, or a configuration map.
    #[cfg(feature = "unstable-schemas")]
    #[derive(schemars::JsonSchema)]
    #[serde(untagged)]
    pub enum FileConfigShape<T> {
        None,
        Path(PathBuf),
        Paths(Vec<PathBuf>),
        Config(FileConfig<T>),
    }

    /// Deserializes nothing, a path, a list of paths, or a configuration map into a [`FileConfig`].
    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<FileConfig<T>, D::Error>
    where
        T: Deserialize<'de> + Default,
        D: Deserializer<'de>,
    {
        struct PathOrConfigVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for PathOrConfigVisitor<T>
        where
            T: Deserialize<'de> + Default,
        {
            type Value = FileConfig<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a path string, a list of path strings, or a configuration map with \
                     `paths` and/or `sources`",
                )
            }

            fn visit_unit<E: de::Error>(self) -> Result<FileConfig<T>, E> {
                Ok(FileConfig::default())
            }

            fn visit_none<E: de::Error>(self) -> Result<FileConfig<T>, E> {
                Ok(FileConfig::default())
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<FileConfig<T>, E> {
                Ok(FileConfig::new(vec![PathBuf::from(value)]))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<FileConfig<T>, E> {
                Ok(FileConfig::new(vec![PathBuf::from(value)]))
            }

            fn visit_seq<S: SeqAccess<'de>>(self, seq: S) -> Result<FileConfig<T>, S::Error> {
                Deserialize::deserialize(SeqAccessDeserializer::new(seq)).map(FileConfig::new)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<FileConfig<T>, M::Error> {
                FileConfig::<T>::deserialize(MapAccessDeserializer::new(map))
            }
        }

        deserializer.deserialize_any(PathOrConfigVisitor(PhantomData))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct FileConfig<T> {
    /// A list of file paths
    #[serde(default, deserialize_with = "one_or_many::deserialize")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "one_or_many::OneOrMany<PathBuf>")
    )]
    pub paths: Vec<PathBuf>,
    /// A list of directories whose subdirectories are each published under the subdirectory's name
    #[serde(default, deserialize_with = "one_or_many::deserialize")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "one_or_many::OneOrMany<PathBuf>")
    )]
    pub collections: Vec<PathBuf>,
    /// A map of source IDs to file paths or config objects
    #[serde(default)]
    pub sources: BTreeMap<String, FileConfigSrc>,
    /// Any customizations related to the specifics of the configuration section
    #[serde(flatten)]
    pub custom: T,
}

impl<T: Serialize> Serialize for FileConfig<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        if !self.paths.is_empty() {
            map.serialize_entry("paths", &self.paths)?;
        }
        if !self.collections.is_empty() {
            map.serialize_entry("collections", &self.collections)?;
        }
        if !self.sources.is_empty() {
            map.serialize_entry("sources", &self.sources)?;
        }
        let custom = serde_json::to_value(&self.custom).map_err(S::Error::custom)?;
        let custom = custom.as_object().ok_or_else(|| {
            S::Error::custom("a flattened file-source configuration must serialize as an object")
        })?;
        for (key, value) in custom {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

impl<T: Default> FileConfig<T> {
    #[must_use]
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            ..Self::default()
        }
    }
}

impl<T: ConfigurationLivecycleHooks> FileConfig<T> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
            && self.collections.is_empty()
            && self.sources.is_empty()
            && self.get_unrecognized_keys().is_empty()
    }

    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// The directories directly inside a collection, sorted by name, as `(name, path)` pairs.
///
/// Files and hidden directories are skipped.
#[cfg(any(
    feature = "_file_kinds",
    feature = "resources",
    feature = "resources",
    feature = "resources"
))]
pub fn subdirectories(collection: &Path) -> std::io::Result<Vec<(String, PathBuf)>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(collection)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() && !name.starts_with('.') {
            found.push((name, path));
        }
    }
    found.sort();
    Ok(found)
}

#[cfg(feature = "_tiles")]
impl<T: TileSourceConfiguration> FileConfig<T> {
    /// The kind level cache bounds over the top level ones.
    #[must_use]
    pub fn cache_or(&self, global: CachePolicy) -> CachePolicy {
        self.custom.cache().or(global)
    }
}

impl<T: ConfigurationLivecycleHooks> ConfigurationLivecycleHooks for FileConfig<T> {
    async fn finalize(&mut self) -> ConfigFileResult<()> {
        if self.is_default() {
            return Ok(());
        }
        self.custom.finalize().await
    }
}

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, PartialEq, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "unstable-schemas", schemars(untagged))]
pub enum FileConfigSrc {
    Path(PathBuf),
    Obj(Box<FileConfigSource>),
}

impl Serialize for FileConfigSrc {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Path(path) => sanitized_source_path(path).serialize(serializer),
            Self::Obj(source) => {
                let mut source = (**source).clone();
                source.path = sanitized_source_path(&source.path);
                source.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for FileConfigSrc {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FileConfigSrcVisitor;

        impl<'de> Visitor<'de> for FileConfigSrcVisitor {
            type Value = FileConfigSrc;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a path string or a configuration map with a `path` field")
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<FileConfigSrc, E> {
                Ok(FileConfigSrc::Path(PathBuf::from(value)))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<FileConfigSrc, E> {
                Ok(FileConfigSrc::Path(PathBuf::from(value)))
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<FileConfigSrc, M::Error> {
                let obj = FileConfigSource::deserialize(MapAccessDeserializer::new(map))?;
                Ok(FileConfigSrc::Obj(Box::new(obj)))
            }

            // Numbers / booleans / sequences fall through to serde's default `invalid_type`
            // path, which carries a source span via saphyr's deserializer.
        }

        deserializer.deserialize_any(FileConfigSrcVisitor)
    }
}

impl FileConfigSrc {
    #[must_use]
    pub fn into_path(self) -> PathBuf {
        match self {
            Self::Path(p) => p,
            Self::Obj(o) => o.path,
        }
    }

    #[must_use]
    pub fn get_path(&self) -> &PathBuf {
        match self {
            Self::Path(p) => p,
            Self::Obj(o) => &o.path,
        }
    }

    #[must_use]
    pub fn cache_zoom(&self) -> CachePolicy {
        match self {
            Self::Path(_) => CachePolicy::default(),
            Self::Obj(o) => o.cache,
        }
    }

    pub fn abs_path(&self) -> ConfigFileResult<PathBuf> {
        let path = self.get_path();

        #[cfg(feature = "mbtiles")]
        if is_sqlite_memory_uri(path) {
            // Skip canonicalization for in-memory DB URIs
            return Ok(path.clone());
        }

        path.canonicalize()
            .map_err(|e| ConfigFileError::IoError(e, path.clone()))
    }
}

#[cfg(feature = "mbtiles")]
fn is_sqlite_memory_uri(path: &Path) -> bool {
    if let Some(s) = path.to_str() {
        s.starts_with("file:") && s.contains("mode=memory") && s.contains("cache=shared")
    } else {
        false
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct FileConfigSource {
    pub path: PathBuf,
    /// Tile grid this source's tiles are on \[default: `WebMercatorQuad`\]
    ///
    /// One of the grids under the top-level `tile_grids`, or a built-in one.
    /// Only `MBTiles` and `PMTiles` sources can be declared to be on another grid.
    #[cfg(feature = "_tiles")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &"WebMercatorQuad"))]
    pub tile_grid: Option<String>,
    /// MVT->MLT encoder settings for this source.
    /// Overrides source-type and global `convert_to_mlt`.
    #[cfg(feature = "_tiles")]
    #[serde(default)]
    pub convert_to_mlt: Option<MltProcessConfig>,
    /// MLT->MVT conversion settings for this source.
    /// Overrides source-type and global `convert_to_mvt`.
    #[cfg(feature = "_tiles")]
    #[serde(default)]
    pub convert_to_mvt: Option<MvtProcessConfig>,
    /// Hillshade settings for this source.
    ///
    /// Present means the source serves Mapzen *normal* tiles and Martin should bake a hillshade from them.
    /// See the hillshade documentation for the knobs.
    /// Settable per source only, since it describes what this source serves rather than a server-wide policy.
    #[cfg(all(feature = "processing", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_hillshade: Option<HillshadeProcessConfig>,
    /// Trace contour lines from this source's tiles.
    ///
    /// Present means the source serves Mapzen *Terrarium* elevation tiles and Martin should trace contours from them.
    /// See the contour documentation for the knobs.
    /// Settable per source only, since it is tied to what this source serves (elevation data in Terrarium format).
    #[cfg(all(feature = "processing", feature = "_tiles"))]
    #[serde(default)]
    pub convert_to_contour: Option<ContourProcessConfig>,
    /// Zoom-level bounds for tile caching.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "CachePolicyShape"))]
    pub cache: CachePolicy,
    /// `Cache-Control` response header for this source.
    /// Overrides the top-level `cache_control` default.
    #[serde(default)]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub cache_control: Option<CacheControlHeader>,
}

#[cfg(feature = "_tiles")]
pub async fn resolve_files<T: TileSourceConfiguration>(
    config: &mut FileConfig<T>,
    idr: &IdResolver,
    extension: &[&str],
    default_cache: CachePolicy,
    tile_grids: Option<&TileGrids>,
) -> ResolutionResult {
    resolve_int(config, idr, extension, default_cache, tile_grids).await
}

/// How many tile sources are opened at once at startup and on reload.
/// Opening a remote source is a few dependent round trips, so serial opens cost latency times source count.
///
/// FIXME: make this constant dependent on system size (number of cores) and source types (local/remote)
#[cfg(feature = "_tiles")]
pub const MAX_CONCURRENT_SOURCE_INITS: usize = 64;

#[cfg(feature = "_tiles")]
async fn resolve_int<T: TileSourceConfiguration>(
    config: &mut FileConfig<T>,
    idr: &IdResolver,
    extension: &[&str],
    default_cache: CachePolicy,
    tile_grids: Option<&TileGrids>,
) -> ResolutionResult {
    let default_cache = config.cache_or(default_cache);

    let mut warnings = Vec::new();
    let mut configs = BTreeMap::new();
    let mut files: HashMap<PathBuf, PathBuf> = HashMap::new();
    let mut directories = Vec::new();
    let mut planned = Vec::new();

    for (id, source) in mem::take(&mut config.sources) {
        match plan_one_source(
            T::parse_urls(),
            idr,
            &id,
            source,
            &mut files,
            &mut configs,
            default_cache,
            tile_grids,
        ) {
            Ok(p) => planned.push(p),
            Err(err) => warnings.push(TileSourceWarning::SourceError {
                source_id: id,
                error: err.to_string(),
            }),
        }
    }

    for path in mem::take(&mut config.paths) {
        match plan_one_path(
            T::parse_urls(),
            idr,
            extension,
            path.clone(),
            &mut files,
            &mut directories,
            &mut configs,
            default_cache,
        ) {
            Ok(p) => planned.extend(p),
            Err(err) => warnings.push(TileSourceWarning::PathError {
                path,
                error: err.to_string(),
            }),
        }
    }

    let custom = &config.custom;
    let opened = stream::iter(planned)
        .map(|p| async move {
            let result = p.open(custom).await;
            (p, result)
        })
        .buffered(MAX_CONCURRENT_SOURCE_INITS)
        .collect::<Vec<_>>()
        .await;

    let mut results = Vec::new();
    for (p, result) in opened {
        match result {
            Ok(src) => {
                let src = src.boxed_on(p.grid.as_ref());
                p.log_configured();
                if !p.from_sources
                    && let Target::File { path, .. } = &p.target
                {
                    configs.insert(p.id.clone(), FileConfigSrc::Path(path.clone()));
                }
                results.push(src);
            }
            Err(err) => warnings.push(p.warning(&err)),
        }
    }

    config.paths = directories;
    config.sources = configs;

    Ok((results, warnings))
}

#[cfg(feature = "_tiles")]
enum Target {
    Url {
        url: Url,
        /// The configured path, for the warning if the open fails.
        configured: PathBuf,
    },
    File {
        path: PathBuf,
        canonical: PathBuf,
    },
}

/// A source whose id is resolved and whose open is still pending.
#[cfg(feature = "_tiles")]
struct Planned {
    id: String,
    target: Target,
    cache: CachePolicy,
    /// The grid the config declares this source to be on, when it is not Web Mercator.
    grid: Option<TileGrid>,
    /// From `sources` rather than `paths`: failures are reported by id instead of path, and
    /// discovered files only enter the config once they open, so one bad file in a directory
    /// does not take its siblings with it.
    from_sources: bool,
    duplicate: bool,
}

#[cfg(feature = "_tiles")]
impl Planned {
    async fn open<T: TileSourceConfiguration>(
        &self,
        custom: &T,
    ) -> SourceBuildResult<BackendSource> {
        match &self.target {
            Target::Url { url, .. } => {
                custom
                    .new_sources_url(self.id.clone(), url.clone(), self.cache)
                    .await
            }
            Target::File { path, .. } => {
                custom
                    .new_sources(self.id.clone(), path.clone(), self.cache)
                    .await
            }
        }
    }

    fn log_configured(&self) {
        match &self.target {
            Target::Url { url, .. } if self.from_sources => info!(
                source.id = %self.id,
                source.url = %sanitize_url(url),
                source.duplicate = self.duplicate,
                "Configured source"
            ),
            Target::Url { url, .. } => info!(
                source.id = %self.id,
                source.url = %sanitize_url(url),
                "Configured source from URL"
            ),
            Target::File { canonical, .. } if self.from_sources => info!(
                source.id = %self.id,
                source.path = %canonical.display(),
                source.duplicate = self.duplicate,
                "Configured source"
            ),
            Target::File { canonical, .. } => info!(
                source.id = %self.id,
                source.path = %canonical.display(),
                "Configured source"
            ),
        }
    }

    fn warning(&self, err: &SourceBuildError) -> TileSourceWarning {
        if self.from_sources {
            return TileSourceWarning::SourceError {
                source_id: self.id.clone(),
                error: err.to_string(),
            };
        }
        let path = match &self.target {
            Target::Url { configured, .. } => configured.clone(),
            Target::File { path, .. } => path.clone(),
        };
        TileSourceWarning::PathError {
            path,
            error: err.to_string(),
        }
    }
}

/// Resolves the id of one configured source (a URL or a file) and records it, without opening it.
#[cfg(feature = "_tiles")]
#[expect(clippy::too_many_arguments)]
fn plan_one_source(
    parse_urls: bool,
    idr: &IdResolver,
    id: &str,
    source: FileConfigSrc,
    files: &mut HashMap<PathBuf, PathBuf>,
    configs: &mut BTreeMap<String, FileConfigSrc>,
    default_cache: CachePolicy,
    tile_grids: Option<&TileGrids>,
) -> SourceBuildResult<Planned> {
    let cache = source.cache_zoom().or(default_cache);
    let grid = declared_tile_grid(id, &source, tile_grids)?;
    if let Some(url) = parse_url(parse_urls, source.get_path())? {
        let key = source.get_path().clone();
        let duplicate = files.insert(key.clone(), key.clone()).is_some();
        let id = idr.resolve(id, sanitize_url(&url));
        configs.insert(id.clone(), source);
        return Ok(Planned {
            id,
            target: Target::Url {
                url,
                configured: key,
            },
            cache,
            grid,
            from_sources: true,
            duplicate,
        });
    }
    let can = source.abs_path()?;
    let duplicate = files.insert(can.clone(), can.clone()).is_some();
    let id = idr.resolve(id, can.to_string_lossy().to_string());
    configs.insert(id.clone(), source.clone());
    Ok(Planned {
        id,
        target: Target::File {
            path: source.into_path(),
            canonical: can,
        },
        cache,
        grid,
        from_sources: true,
        duplicate,
    })
}

/// The grid a configured source declares, resolved by name.
///
/// Discovered files never declare one.
/// A kind that cannot be served on another grid passes `None` for `tile_grids`, which makes any declaration an error.
#[cfg(feature = "_tiles")]
pub(crate) fn declared_tile_grid(
    id: &str,
    source: &FileConfigSrc,
    tile_grids: Option<&TileGrids>,
) -> ConfigFileResult<Option<TileGrid>> {
    let FileConfigSrc::Obj(obj) = source else {
        return Ok(None);
    };
    let Some(name) = obj.tile_grid.as_deref() else {
        return Ok(None);
    };
    let Some(grids) = tile_grids else {
        return Err(ConfigFileError::TileGridNotSupported {
            what: format!("Source {id}"),
        });
    };
    let grid = grids
        .get(name)
        .ok_or_else(|| ConfigFileError::UnknownTileGrid {
            what: format!("Source {id}"),
            grid: name.to_owned(),
            known: grids.names().join(", "),
        })?;
    Ok((!grid.is_web_mercator()).then(|| grid.clone()))
}

/// Resolves the ids under one configured path (a URL, a file, or a directory) and records them,
/// without opening any source.
#[cfg(feature = "_tiles")]
#[expect(clippy::too_many_arguments)]
fn plan_one_path(
    parse_urls: bool,
    idr: &IdResolver,
    extension: &[&str],
    path: PathBuf,
    files: &mut HashMap<PathBuf, PathBuf>,
    directories: &mut Vec<PathBuf>,
    configs: &mut BTreeMap<String, FileConfigSrc>,
    default_cache: CachePolicy,
) -> SourceBuildResult<Vec<Planned>> {
    if let Some(url) = parse_url(parse_urls, &path)? {
        let target_ext = extension
            .iter()
            .find(|&&e| url.path().rsplit('.').next() == Some(e));
        let Some(ext) = target_ext else {
            // A URL whose path doesn't end with one of the target extensions is treated as
            // a prefix to be discovered by the format-specific reloader (e.g. PmtilesReloader
            // polling `s3://bucket/`). Push it back into `directories` so the rebuilt
            // config preserves it for the reloader to see.
            info!(
                source.url = %sanitize_url(&url),
                "URL does not end with a known extension; treating as a prefix for the reloader to discover"
            );
            directories.push(path);
            return Ok(Vec::new());
        };
        let id = url
            .path_segments()
            .and_then(Iterator::last)
            .and_then(|s| {
                // Strip extension and trailing dot, or keep the original string
                s.strip_suffix(ext)
                    .and_then(|s| s.strip_suffix('.'))
                    .or(Some(s))
            })
            .unwrap_or("web_source");

        let id = idr.resolve(id, sanitize_url(&url));
        configs.insert(id.clone(), FileConfigSrc::Path(path.clone()));
        return Ok(vec![Planned {
            id,
            target: Target::Url {
                url,
                configured: path,
            },
            cache: default_cache,
            grid: None,
            from_sources: false,
            duplicate: false,
        }]);
    }

    if path.is_dir() {
        directories.push(path);
        return Ok(Vec::new());
    }
    if !path.is_file() {
        return Err(SourceBuildError::from(ConfigFileError::InvalidFilePath(
            path.canonicalize().unwrap_or(path),
        )));
    }

    let can = path
        .canonicalize()
        .map_err(|e| ConfigFileError::IoError(e, path.clone()))?;
    if let Some(kept) = files.get(&can) {
        warn!(
            source.path.dropped = %path.display(),
            source.path.kept = %kept.display(),
            "Ignoring duplicate source path: already configured under another path"
        );
        return Ok(Vec::new());
    }
    files.insert(can.clone(), path.clone());
    let id = path.file_stem().map_or_else(
        || "_unknown".to_owned(),
        |s| s.to_string_lossy().to_string(),
    );
    let id = idr.resolve(&id, can.to_string_lossy().to_string());
    Ok(vec![Planned {
        id,
        target: Target::File {
            path,
            canonical: can,
        },
        cache: default_cache,
        grid: None,
        from_sources: false,
        duplicate: false,
    }])
}

#[cfg(feature = "_tiles")]
fn sanitize_url(url: &Url) -> String {
    let mut result = format!("{}://", url.scheme());
    if let Some(host) = url.host_str() {
        result.push_str(host);
    }
    if let Some(port) = url.port() {
        result.push(':');
        result.push_str(&port.to_string());
    }
    result.push_str(url.path());
    result
}

fn sanitized_source_path(path: &Path) -> PathBuf {
    #[cfg(not(feature = "_tiles"))]
    return path.to_path_buf();

    #[cfg(feature = "_tiles")]
    {
        let Ok(location) = SourceLocation::classify_path(path) else {
            return path.to_path_buf();
        };
        let Some(mut url) = location.into_url() else {
            return path.to_path_buf();
        };
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_query(None);
        url.set_fragment(None);
        PathBuf::from(url.as_str())
    }
}

#[cfg(feature = "_tiles")]
fn parse_url(is_enabled: bool, path: &Path) -> Result<Option<Url>, ConfigFileError> {
    if !is_enabled {
        return Ok(None);
    }
    Ok(SourceLocation::classify_path(path)?.into_url())
}

/// Cache configuration for a tile source. Currently holds zoom-level bounds;
/// may be extended with additional cache settings in the future.
///
/// Accepts either a struct with zoom bounds or the string `"disable"` to disable caching:
/// ```yaml
/// cache: disable
/// ```
///
/// ```yaml
/// cache:
///   minzoom: 0
///   maxzoom: 10
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CachePolicy {
    #[serde(flatten)]
    zoom: CacheZoomRange,
}

#[cfg(feature = "unstable-schemas")]
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
#[expect(dead_code, reason = "schema generator sees this through `with = ...`")]
pub(crate) enum CachePolicyShape {
    Disable(DisableLiteral),
    Policy(CachePolicy),
}

#[cfg(feature = "unstable-schemas")]
#[derive(serde::Serialize, schemars::JsonSchema)]
#[expect(dead_code, reason = "schema-only, never constructed")]
pub(crate) enum DisableLiteral {
    #[serde(rename = "disable")]
    Disable,
}

impl CachePolicy {
    /// Creates a new `CachePolicy` with the given zoom range.
    #[must_use]
    pub const fn new(zoom: CacheZoomRange) -> Self {
        Self { zoom }
    }

    /// Creates a disabled `CachePolicy` where caching is turned off.
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            zoom: CacheZoomRange::disabled(),
        }
    }

    /// Returns the zoom-level bounds for caching.
    #[must_use]
    pub const fn zoom(self) -> CacheZoomRange {
        self.zoom
    }

    /// Returns `true` if no cache bounds are configured.
    #[must_use]
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde skip_serializing_if requires &self"
    )]
    pub const fn is_empty(&self) -> bool {
        self.zoom.is_empty()
    }

    /// Fills in any `None` fields from `other`.
    /// A disabled cache policy (with both bounds set) is not overridden by defaults.
    #[must_use]
    pub fn or(self, other: Self) -> Self {
        Self {
            zoom: self.zoom.or(other.zoom),
        }
    }
}

impl<'de> Deserialize<'de> for CachePolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Inner {
            #[serde(flatten, default)]
            zoom: CacheZoomRange,
        }

        struct CachePolicyVisitor;

        impl<'de> Visitor<'de> for CachePolicyVisitor {
            type Value = CachePolicy;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "either the literal `disable` or a zoom range (e.g. `{ minzoom: 0, maxzoom: 14 }`)",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<CachePolicy, E> {
                if value == "disable" {
                    Ok(CachePolicy::disabled())
                } else {
                    Err(E::custom(format!(
                        "invalid cache policy string {value:?}; the only accepted string form is `disable`"
                    )))
                }
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<CachePolicy, E> {
                self.visit_str(&value)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<CachePolicy, M::Error> {
                let inner = Inner::deserialize(MapAccessDeserializer::new(map))?;
                Ok(CachePolicy { zoom: inner.zoom })
            }
        }

        deserializer.deserialize_any(CachePolicyVisitor)
    }
}

/// Global-level cache configuration with both size limits and zoom-level bounds.
///
/// Used at the root of the config file:
/// ```yaml
/// cache:
///   size_mb: 512
///   tile_size_mb: 256
///   expiry: 1h
///   idle_timeout: 15m
///   tile_expiry: 30m
///   tile_idle_timeout: 5m
///   minzoom: 0
///   maxzoom: 20
/// ```
///
/// Or disabled entirely:
/// ```yaml
/// cache: disable
/// ```
#[serde_with::skip_serializing_none]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct GlobalCacheConfig {
    /// Total amount of cache we use \[default: 512, 0 to disable\]
    /// By default, this is split up between:
    /// - Tiles 50% -> 256 MB
    /// - Pmtiles' directories 25% -> 128 MB
    /// - Fonts 12.5% -> 64 MB
    /// - Sprites 12.5% -> 64 MB
    ///
    /// How the cache works internally is unstable and may change to improve performance/efficiency.
    /// For example, we may change the split between sources to improve efficiency.
    ///
    /// Specify each cache size individually for finer cache size control:
    /// - Tiles: `cache.tile_size_mb`
    /// - Pmtiles: `pmtiles.directory_cache.size_mb`
    /// - Fonts: `fonts.cache.size_mb`
    /// - Sprites: `sprites.cache.size_mb`
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &512u64))]
    pub size_mb: Option<u64>,
    /// Allows overriding the size of the tile cache.
    /// Defaults to `cache.size_mb` / 2
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &256u64))]
    pub tile_size_mb: Option<u64>,
    /// Maximum lifetime for all cache entries (time-to-live from creation).
    /// Entries are evicted after this duration regardless of access.
    /// Supports human-readable formats: "1h", "30m", "1d", "3600s".
    /// default: null (no expiry, entries only evicted by size pressure)
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"1h")
    )]
    pub expiry: Option<Duration>,
    /// Maximum idle time for all cache entries (time-to-idle since last access).
    /// Entries are evicted if not accessed within this duration.
    /// default: null (no idle timeout)
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"30m")
    )]
    pub idle_timeout: Option<Duration>,
    /// Tile-specific TTL override. Takes precedence over `cache.expiry` for tiles.
    /// default: null (inherits from `cache.expiry`)
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"1h")
    )]
    pub tile_expiry: Option<Duration>,
    /// Tile-specific idle timeout override. Takes precedence over `cache.idle_timeout` for tiles.
    /// default: null (inherits from `cache.idle_timeout`)
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"30m")
    )]
    pub tile_idle_timeout: Option<Duration>,
    #[serde(flatten)]
    zoom: CacheZoomRange,
}

impl GlobalCacheConfig {
    /// Creates a disabled `GlobalCacheConfig` with size 0 and minzoom > maxzoom.
    #[must_use]
    pub const fn disabled() -> Self {
        Self {
            size_mb: Some(0),
            tile_size_mb: Some(0),
            expiry: None,
            idle_timeout: None,
            tile_expiry: None,
            tile_idle_timeout: None,
            zoom: CacheZoomRange::disabled(),
        }
    }

    /// Returns the zoom-level bounds as a [`CachePolicy`].
    #[must_use]
    pub const fn policy(self) -> CachePolicy {
        CachePolicy::new(self.zoom)
    }

    /// Returns `true` if no cache settings are configured.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.size_mb.is_none()
            && self.tile_size_mb.is_none()
            && self.expiry.is_none()
            && self.idle_timeout.is_none()
            && self.tile_expiry.is_none()
            && self.tile_idle_timeout.is_none()
            && self.zoom.is_empty()
    }
}

#[cfg(feature = "unstable-schemas")]
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
#[expect(dead_code, reason = "schema generator sees this through `with = ...`")]
pub(crate) enum GlobalCacheConfigShape {
    Disable(DisableLiteral),
    Config(GlobalCacheConfig),
}

impl<'de> Deserialize<'de> for GlobalCacheConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Inner struct that handles the map case via the derive - we still get good error
        // messages (with spans) for unknown fields and type mismatches inside it.
        #[serde_with::skip_serializing_none]
        #[derive(Deserialize)]
        struct Inner {
            size_mb: Option<u64>,
            tile_size_mb: Option<u64>,
            #[serde(default, with = "humantime_serde")]
            expiry: Option<Duration>,
            #[serde(default, with = "humantime_serde")]
            idle_timeout: Option<Duration>,
            #[serde(default, with = "humantime_serde")]
            tile_expiry: Option<Duration>,
            #[serde(default, with = "humantime_serde")]
            tile_idle_timeout: Option<Duration>,
            #[serde(flatten, default)]
            zoom: CacheZoomRange,
        }

        struct GlobalCacheVisitor;

        impl<'de> Visitor<'de> for GlobalCacheVisitor {
            type Value = GlobalCacheConfig;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "either the literal `disable` or a cache configuration map (e.g. `{ size_mb: 512, tile_size_mb: 256 }`)",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<GlobalCacheConfig, E> {
                if value == "disable" {
                    Ok(GlobalCacheConfig::disabled())
                } else {
                    Err(E::custom(format!(
                        "invalid cache config string {value:?}; the only accepted string form is `disable`"
                    )))
                }
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<GlobalCacheConfig, E> {
                self.visit_str(&value)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<GlobalCacheConfig, M::Error> {
                let inner = Inner::deserialize(MapAccessDeserializer::new(map))?;
                Ok(GlobalCacheConfig {
                    size_mb: inner.size_mb,
                    tile_size_mb: inner.tile_size_mb,
                    expiry: inner.expiry,
                    idle_timeout: inner.idle_timeout,
                    tile_expiry: inner.tile_expiry,
                    tile_idle_timeout: inner.tile_idle_timeout,
                    zoom: inner.zoom,
                })
            }
        }

        deserializer.deserialize_any(GlobalCacheVisitor)
    }
}

/// Cache size configuration for a source type (sprites, fonts, pmtiles).
///
/// Used at the source-type level:
/// ```yaml
/// sprites:
///   cache:
///     size_mb: 64
/// ```
///
/// Or disabled entirely:
/// ```yaml
/// sprites:
///   cache: disable
/// ```
#[serde_with::skip_serializing_none]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CacheSizeConfig {
    /// Size of the cache in MB (0 to disable).
    /// default: inherits from `cache.size_mb` (with a per-source split)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &64u64))]
    pub size_mb: Option<u64>,
    /// Maximum lifetime for cache entries.
    /// default: null (inherits from `cache.expiry`)
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"1h")
    )]
    pub expiry: Option<Duration>,
    /// Maximum idle time for cache entries.
    /// default: null (inherits from `cache.idle_timeout`)
    #[serde(default, with = "humantime_serde")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "Option<String>", example = &"30m")
    )]
    pub idle_timeout: Option<Duration>,
}

impl CacheSizeConfig {
    /// Returns `true` if no cache settings are configured.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.size_mb.is_none() && self.expiry.is_none() && self.idle_timeout.is_none()
    }
}

#[cfg(feature = "unstable-schemas")]
#[derive(serde::Serialize, schemars::JsonSchema)]
#[serde(untagged)]
#[expect(dead_code, reason = "schema generator sees this through `with = ...`")]
pub(crate) enum CacheSizeConfigShape {
    Disable(DisableLiteral),
    Config(CacheSizeConfig),
}

impl<'de> Deserialize<'de> for CacheSizeConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[serde_with::skip_serializing_none]
        #[derive(Deserialize)]
        struct Inner {
            size_mb: Option<u64>,
            #[serde(default, with = "humantime_serde")]
            expiry: Option<Duration>,
            #[serde(default, with = "humantime_serde")]
            idle_timeout: Option<Duration>,
        }

        struct CacheSizeVisitor;

        impl<'de> Visitor<'de> for CacheSizeVisitor {
            type Value = CacheSizeConfig;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "either the literal `disable` or a cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)",
                )
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<CacheSizeConfig, E> {
                if value == "disable" {
                    Ok(CacheSizeConfig {
                        size_mb: Some(0),
                        expiry: None,
                        idle_timeout: None,
                    })
                } else {
                    Err(E::custom(format!(
                        "invalid cache config string {value:?}; the only accepted string form is `disable`"
                    )))
                }
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<CacheSizeConfig, E> {
                self.visit_str(&value)
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<CacheSizeConfig, M::Error> {
                let inner = Inner::deserialize(MapAccessDeserializer::new(map))?;
                Ok(CacheSizeConfig {
                    size_mb: inner.size_mb,
                    expiry: inner.expiry,
                    idle_timeout: inner.idle_timeout,
                })
            }
        }

        deserializer.deserialize_any(CacheSizeVisitor)
    }
}

pub type UnrecognizedKeys = HashSet<String>;

pub fn copy_unrecognized_keys_from_config(
    result: &mut UnrecognizedKeys,
    prefix: &str,
    unrecognized: &UnrecognizedValues,
) {
    result.extend(unrecognized.keys().map(|k| format!("{prefix}{k}")));
}

#[cfg(test)]
mod deserialize_tests {
    use serde::Deserialize;

    use super::*;
    use crate::config::test_helpers::{parse_yaml, render_failure};

    /// Inner config used to instantiate `FileConfig<T>` in success-path
    /// tests without depending on a real source-type config.
    #[derive(
        Clone,
        Debug,
        Default,
        Deserialize,
        PartialEq,
        Serialize,
        CollectUnrecognizedKeys,
        ConfigurationLivecycleHooks,
    )]
    struct TestCustom {
        #[serde(default)]
        flag: bool,
    }

    // Failure-path tests run through the full `parse_config` pipeline using realistic
    // `Config` fields (e.g. `pmtiles:` for `FileConfig`, `mbtiles.sources` for
    // `FileConfigSrc`, `cache:` for the cache deserializers) so each snapshot mirrors what
    // the user sees on the command line.

    // ----- FileConfig<T> -----

    #[derive(Debug, PartialEq, Deserialize)]
    struct Section(
        #[serde(deserialize_with = "path_or_config::deserialize")] FileConfig<TestCustom>,
    );

    fn parse_section(yaml: &str) -> FileConfig<TestCustom> {
        parse_yaml::<Section>(yaml).0
    }

    #[test]
    fn path_or_config_null_is_default() {
        assert_eq!(parse_section("null"), FileConfig::default());
    }

    #[test]
    fn path_or_config_string_is_one_path() {
        assert_eq!(
            parse_section("/tmp/tiles"),
            FileConfig::new(vec![PathBuf::from("/tmp/tiles")])
        );
    }

    #[test]
    fn path_or_config_seq_is_paths() {
        assert_eq!(
            parse_section("[/a, /b]"),
            FileConfig::new(vec![PathBuf::from("/a"), PathBuf::from("/b")])
        );
    }

    #[test]
    fn path_or_config_map_is_config() {
        let cfg = parse_section("{ paths: [/a], flag: true }");
        assert_eq!(cfg.paths, vec![PathBuf::from("/a")]);
        assert!(cfg.custom.flag);
    }

    #[test]
    #[cfg(feature = "pmtiles")]
    fn path_or_config_rejects_integer() {
        insta::assert_snapshot!(render_failure("pmtiles: 42\n"), @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: integer `42`, expected a path string, a list of path
          │ strings, or a configuration map with `paths` and/or `sources`
           ╭─[config.yaml:1:1]
         1 │ pmtiles: 42
           · ───┬───
           ·    ╰── invalid type: integer `42`, expected a path string, a list of path strings, or a configuration map with `paths` and/or `sources`
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        ");
    }

    #[test]
    #[cfg(feature = "pmtiles")]
    fn path_or_config_rejects_bool() {
        insta::assert_snapshot!(render_failure("pmtiles: true\n"), @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: boolean `true`, expected a path string, a list of path
          │ strings, or a configuration map with `paths` and/or `sources`
           ╭─[config.yaml:1:1]
         1 │ pmtiles: true
           · ───┬───
           ·    ╰── invalid type: boolean `true`, expected a path string, a list of path strings, or a configuration map with `paths` and/or `sources`
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        ");
    }

    #[test]
    #[cfg(feature = "pmtiles")]
    fn path_or_config_path_list_with_nested_map_fails() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                pmtiles:
                  paths:
                    - { not_a_path: true }
            "}),
            @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × expected string scalar
           ╭─[config.yaml:3:7]
         2 │   paths:
         3 │     - { not_a_path: true }
           ·       ┬
           ·       ╰── expected string scalar
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "
        );
    }

    #[test]
    fn file_config_src_string_is_path() {
        let cfg = parse_yaml::<FileConfigSrc>("/tmp/tile.pmtiles");
        assert_eq!(cfg, FileConfigSrc::Path(PathBuf::from("/tmp/tile.pmtiles")));
    }

    #[test]
    fn file_config_src_map_is_obj() {
        let cfg = parse_yaml::<FileConfigSrc>("{ path: /tmp/tile.pmtiles }");
        let FileConfigSrc::Obj(obj) = cfg else {
            panic!("expected Obj variant");
        };
        assert_eq!(obj.path, PathBuf::from("/tmp/tile.pmtiles"));
    }

    #[cfg(feature = "_tiles")]
    #[test]
    fn file_config_src_serialization_redacts_remote_url_credentials() {
        let source = FileConfigSrc::Path(PathBuf::from(
            "https://user:password@example.com/image.tif?token=secret#fragment",
        ));
        assert_eq!(
            serde_json::to_value(source).unwrap(),
            serde_json::Value::String("https://example.com/image.tif".to_owned())
        );

        let source = FileConfigSrc::Obj(Box::new(FileConfigSource {
            path: PathBuf::from("s3://user:password@bucket/image.tif?token=secret#fragment"),
            ..FileConfigSource::default()
        }));
        assert_eq!(
            serde_json::to_value(source).unwrap()["path"],
            "s3://bucket/image.tif"
        );
    }

    #[test]
    #[cfg(feature = "mbtiles")]
    fn file_config_src_rejects_integer() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                mbtiles:
                  sources:
                    foo: 5
            "}),
            @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: integer `5`, expected a path string or a configuration map
          │ with a `path` field
           ╭─[config.yaml:3:5]
         2 │   sources:
         3 │     foo: 5
           ·     ─┬─
           ·      ╰── invalid type: integer `5`, expected a path string or a configuration map with a `path` field
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "
        );
    }

    #[test]
    #[cfg(feature = "mbtiles")]
    fn file_config_src_rejects_bool() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                mbtiles:
                  sources:
                    foo: true
            "}),
            @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: boolean `true`, expected a path string or a configuration
          │ map with a `path` field
           ╭─[config.yaml:3:5]
         2 │   sources:
         3 │     foo: true
           ·     ─┬─
           ·      ╰── invalid type: boolean `true`, expected a path string or a configuration map with a `path` field
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "
        );
    }

    #[test]
    #[cfg(feature = "mbtiles")]
    fn file_config_src_rejects_sequence() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                mbtiles:
                  sources:
                    foo: [a, b]
            "}),
            @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: sequence, expected a path string or a configuration map with
          │ a `path` field
           ╭─[config.yaml:3:5]
         2 │   sources:
         3 │     foo: [a, b]
           ·     ─┬─
           ·      ╰── invalid type: sequence, expected a path string or a configuration map with a `path` field
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "
        );
    }

    // ----- GlobalCacheConfig (top-level `cache:` key) -----

    #[test]
    fn global_cache_disable_string() {
        let cfg = parse_yaml::<GlobalCacheConfig>("disable");
        assert_eq!(cfg, GlobalCacheConfig::disabled());
    }

    #[test]
    fn global_cache_map() {
        let cfg = parse_yaml::<GlobalCacheConfig>("{ size_mb: 512, tile_size_mb: 256 }");
        assert_eq!(cfg.size_mb, Some(512));
        assert_eq!(cfg.tile_size_mb, Some(256));
    }

    #[test]
    fn global_cache_rejects_other_string() {
        insta::assert_snapshot!(render_failure("cache: enable\n"), @r#"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid cache config string "enable"; the only accepted string form is
          │ `disable`
           ╭─[config.yaml:1:8]
         1 │ cache: enable
           ·        ───┬──
           ·           ╰── invalid cache config string "enable"; the only accepted string form is `disable`
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "#);
    }

    #[test]
    fn global_cache_rejects_integer() {
        insta::assert_snapshot!(render_failure("cache: 42\n"), @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: integer `42`, expected either the literal `disable` or a
          │ cache configuration map (e.g. `{ size_mb: 512, tile_size_mb: 256 }`)
           ╭─[config.yaml:1:1]
         1 │ cache: 42
           · ──┬──
           ·   ╰── invalid type: integer `42`, expected either the literal `disable` or a cache configuration map (e.g. `{ size_mb: 512, tile_size_mb: 256 }`)
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        ");
    }

    // ----- CacheSizeConfig (per-section `cache:` block) -----

    #[test]
    fn cache_size_disable_string() {
        let cfg = parse_yaml::<CacheSizeConfig>("disable");
        assert_eq!(cfg.size_mb, Some(0));
        assert_eq!(cfg.expiry, None);
    }

    #[test]
    fn cache_size_map() {
        let cfg = parse_yaml::<CacheSizeConfig>("{ size_mb: 64, expiry: 1h }");
        assert_eq!(cfg.size_mb, Some(64));
        assert_eq!(cfg.expiry, Some(Duration::from_hours(1)));
    }

    #[test]
    #[cfg(feature = "resources")]
    fn cache_size_rejects_other_string() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                sprites:
                  cache: yes
            "}),
            @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: boolean `true`, expected either the literal `disable` or a
          │ cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)
           ╭─[config.yaml:2:3]
         1 │ sprites:
         2 │   cache: yes
           ·   ──┬──
           ·     ╰── invalid type: boolean `true`, expected either the literal `disable` or a cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "
        );
    }

    #[test]
    #[cfg(feature = "resources")]
    fn cache_size_rejects_integer() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                sprites:
                  cache: 42
            "}),
            @"
        martin::config::yaml (https://maplibre.org/martin/config-file/)

          × invalid type: integer `42`, expected either the literal `disable` or a
          │ cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)
           ╭─[config.yaml:2:3]
         1 │ sprites:
         2 │   cache: 42
           ·   ──┬──
           ·     ╰── invalid type: integer `42`, expected either the literal `disable` or a cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)
           ╰────
          help: Check the highlighted token in your YAML. The error usually indicates
                a mismatched type or an unexpected shape.
        "
        );
    }

    // ----- CachePolicy (constructed internally, not surfaced as a config-tree field) -----
    //
    // `CachePolicy` is built from `CacheZoomRange` derived from per-source defaults; it is
    // not addressable via a top-level YAML path. We exercise the deserializer directly here
    // and rely on the `cache:` and per-source `cache:` block tests above to cover the
    // user-visible diagnostic surface.

    #[cfg(feature = "mbtiles")]
    #[test]
    fn cache_or_layers_the_kind_level_over_the_global_one() {
        use crate::config::file::mbtiles::MbtConfig;

        let global = CachePolicy::new(CacheZoomRange::new(Some(1), Some(10)));
        let kind = FileConfig {
            custom: MbtConfig {
                cache: CachePolicy::new(CacheZoomRange::new(None, Some(5))),
                ..MbtConfig::default()
            },
            ..FileConfig::default()
        };
        assert_eq!(
            kind.cache_or(global).zoom(),
            CacheZoomRange::new(Some(1), Some(5))
        );
        assert_eq!(
            FileConfig::<MbtConfig>::default().cache_or(global).zoom(),
            global.zoom()
        );
    }

    #[test]
    fn cache_policy_disable_string() {
        let cfg = parse_yaml::<CachePolicy>("disable");
        assert_eq!(cfg, CachePolicy::disabled());
    }

    #[test]
    fn cache_policy_map() {
        let cfg = parse_yaml::<CachePolicy>("{ minzoom: 0, maxzoom: 14 }");
        let dumped = serde_saphyr::to_string(&cfg).unwrap();
        assert!(dumped.contains("minzoom: 0"), "got: {dumped}");
        assert!(dumped.contains("maxzoom: 14"), "got: {dumped}");
    }
}

#[cfg(all(test, feature = "mbtiles"))]
mod mbtiles_tests {
    use super::*;
    use crate::config::file::tiles::mbtiles::MbtConfig;
    use crate::config::primitives::IdResolver;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn invalid_path_warns_instead_of_failing() {
        let invalid_path = PathBuf::from("/nonexistent/path/");
        let invalid_source = PathBuf::from("/nonexistent/path/to/file.mbtiles");
        let mut file_sources = BTreeMap::new();
        file_sources.insert(
            "test_source".to_owned(),
            FileConfigSrc::Path(invalid_source.clone()),
        );
        let mut config = FileConfig {
            paths: vec![invalid_path.clone()],
            collections: Vec::new(),
            sources: file_sources,
            custom: MbtConfig::default(),
        };

        let idr = IdResolver::new(&[]);
        let result = resolve_files(
            &mut config,
            &idr,
            &["mbtiles"],
            CachePolicy::default(),
            None,
        )
        .await;

        let (sources, warnings) = result.unwrap();
        assert_eq!(sources.len(), 0);
        assert_eq!(warnings.len(), 2);
    }
}

#[cfg(all(test, feature = "_tiles"))]
mod plan_one_path_tests {
    use super::*;
    use crate::config::primitives::IdResolver;

    fn plan(url: &str) -> (Vec<Planned>, Vec<PathBuf>) {
        let idr = IdResolver::new(&[]);
        let mut files = HashMap::new();
        let mut directories = Vec::new();
        let mut configs = BTreeMap::new();

        let planned = plan_one_path(
            true,
            &idr,
            &["tif", "tiff"],
            PathBuf::from(url),
            &mut files,
            &mut directories,
            &mut configs,
            CachePolicy::default(),
        )
        .expect("plan_one_path should accept a well-formed URL");

        (planned, directories)
    }

    #[test]
    fn a_path_segment_merely_ending_in_the_extension_letters_is_not_a_match() {
        let (planned, directories) = plan("https://example.com/some/motif");
        assert!(
            planned.is_empty(),
            "'motif' must not be misdetected as ending in the 'tif' extension"
        );
        assert_eq!(
            directories,
            vec![PathBuf::from("https://example.com/some/motif")]
        );
    }

    #[test]
    fn a_url_ending_with_a_known_extension_is_a_match() {
        let (planned, directories) = plan("https://example.com/image.tif");
        assert_eq!(planned.len(), 1);
        assert_eq!(directories, [] as [PathBuf; 0]);
    }
}

#[cfg(all(test, feature = "pmtiles"))]
mod pmtiles_tests {
    use super::*;
    use crate::config::file::tiles::pmtiles::PmtConfig;
    use crate::config::primitives::IdResolver;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn invalid_path_warns_instead_of_failing() {
        let invalid_path = PathBuf::from("/nonexistent/path/");
        let invalid_source = PathBuf::from("/nonexistent/path/to/file.pmtiles");
        let mut file_sources = BTreeMap::new();
        file_sources.insert(
            "test_source".to_owned(),
            FileConfigSrc::Path(invalid_source.clone()),
        );
        let mut config = FileConfig {
            paths: vec![invalid_path.clone()],
            collections: Vec::new(),
            sources: file_sources,
            custom: PmtConfig::default(),
        };

        let idr = IdResolver::new(&[]);
        let result = resolve_files(
            &mut config,
            &idr,
            &["pmtiles"],
            CachePolicy::default(),
            None,
        )
        .await;

        let (sources, warnings) = result.unwrap();
        assert_eq!(sources.len(), 0);
        assert_eq!(warnings.len(), 2);
    }
}
