use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::mem;
#[cfg(feature = "_tiles")]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use martin_core::CacheZoomRange;
#[cfg(feature = "_tiles")]
use martin_core::tiles::BoxedSource;
use serde::{Deserialize, Serialize};
#[cfg(feature = "_tiles")]
use tracing::{info, warn};
#[cfg(feature = "_tiles")]
use url::Url;

#[cfg(feature = "_tiles")]
use crate::config::file::TileSourceWarning;
use crate::config::file::{ConfigFileError, ConfigFileResult};
#[cfg(feature = "_tiles")]
use crate::config::primitives::IdResolver;
use crate::config::primitives::OptOneMany;
#[cfg(feature = "_tiles")]
use crate::{MartinError, MartinResult};

/// Lifecycle hooks for configuring the application
///
/// The hooks are guaranteed called in the following order:
/// 1. `finalize`
/// 2. `get_unrecognized_keys`
pub trait ConfigurationLivecycleHooks: Clone + Debug + Default + PartialEq + Send {
    /// Finalize configuration discovery and patch old values
    ///
    /// In practice, this method is only implemented on a path of the config if a value or a value in the path below it needs to be finalized
    fn finalize(&mut self) -> ConfigFileResult<()> {
        Ok(())
    }

    /// Iterates over all unrecognized (present, but not expected) keys in the configuration
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys;

    /// Returns all results of the [`Self::get_unrecognized_keys`], but with a given prefix
    fn get_unrecognized_keys_with_prefix(&self, prefix: &str) -> UnrecognizedKeys {
        self.get_unrecognized_keys()
            .into_iter()
            .map(|key| format!("{prefix}{key}"))
            .collect()
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

    /// Asynchronously creates a new `BoxedSource` from a **local** file `path` using the given `id`.
    ///
    /// This function is called for each discovered file path that is not a URL.
    /// `cache` contains per-source zoom bounds, already merged with defaults.
    fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> impl Future<Output = MartinResult<BoxedSource>> + Send;

    /// Asynchronously creates a new `BoxedSource` from a **remote** `url` using the given `id`.
    ///
    /// This function is called for each discovered source path that is a valid URL.
    /// `cache` contains per-source zoom bounds, already merged with defaults.
    fn new_sources_url(
        &self,
        id: String,
        url: Url,
        cache: CachePolicy,
    ) -> impl Future<Output = MartinResult<BoxedSource>> + Send;
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileConfigEnum<T> {
    #[default]
    None,
    Path(PathBuf),
    Paths(Vec<PathBuf>),
    Config(FileConfig<T>),
}

impl<T: ConfigurationLivecycleHooks> FileConfigEnum<T> {
    #[must_use]
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self::new_extended(paths, BTreeMap::new(), T::default())
    }

    #[must_use]
    pub fn new_extended(
        paths: Vec<PathBuf>,
        configs: BTreeMap<String, FileConfigSrc>,
        custom: T,
    ) -> Self {
        // Collapse to the simpler `Path` / `Paths` / `None` variants only when both `configs`
        // and `custom` carry no information; otherwise preserve `custom` by emitting `Config`.
        // Without this, custom settings such as `pmtiles.reload_interval_secs` would silently
        // disappear after `resolve_files` rebuilds the enum for an empty source set.
        if configs.is_empty() && custom == T::default() {
            match paths.len() {
                0 => Self::None,
                1 => Self::Path(paths.into_iter().next().expect("one path exists")),
                _ => Self::Paths(paths),
            }
        } else {
            Self::Config(FileConfig {
                paths: OptOneMany::new(paths),
                sources: if configs.is_empty() {
                    None
                } else {
                    Some(configs)
                },
                custom,
            })
        }
    }

    #[must_use]
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::None => true,
            Self::Path(_) => false,
            Self::Paths(v) => v.is_empty(),
            Self::Config(c) => c.is_empty(),
        }
    }

    pub fn extract_file_config(&mut self) -> Option<FileConfig<T>> {
        match self {
            Self::None => None,
            Self::Path(path) => Some(FileConfig {
                paths: OptOneMany::One(mem::take(path)),
                ..FileConfig::default()
            }),
            Self::Paths(paths) => Some(FileConfig {
                paths: OptOneMany::Many(mem::take(paths)),
                ..Default::default()
            }),
            Self::Config(cfg) => Some(mem::take(cfg)),
        }
    }

    /// convert path/paths and the config enums
    #[must_use]
    pub fn into_config(self) -> Self {
        match self {
            Self::Path(path) => Self::Config(FileConfig {
                paths: OptOneMany::One(path),
                sources: None,
                custom: T::default(),
            }),
            Self::Paths(paths) => Self::Config(FileConfig {
                paths: OptOneMany::Many(paths),
                sources: None,
                custom: T::default(),
            }),
            c => c,
        }
    }
}

impl<T: ConfigurationLivecycleHooks> ConfigurationLivecycleHooks for FileConfigEnum<T> {
    fn finalize(&mut self) -> ConfigFileResult<()> {
        if let Self::Config(cfg) = self {
            cfg.finalize()
        } else {
            Ok(())
        }
    }

    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        if let Self::Config(cfg) = self {
            cfg.get_unrecognized_keys()
        } else {
            UnrecognizedKeys::new()
        }
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfig<T> {
    /// A list of file paths
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub paths: OptOneMany<PathBuf>,
    /// A map of source IDs to file paths or config objects
    pub sources: Option<BTreeMap<String, FileConfigSrc>>,
    /// Any customizations related to the specifics of the configuration section
    #[serde(flatten)]
    pub custom: T,
}

impl<T: ConfigurationLivecycleHooks> FileConfig<T> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_none() && self.sources.is_none() && self.get_unrecognized_keys().is_empty()
    }
}

impl<T: ConfigurationLivecycleHooks> ConfigurationLivecycleHooks for FileConfig<T> {
    fn finalize(&mut self) -> ConfigFileResult<()> {
        self.custom.finalize()
    }
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.custom.get_unrecognized_keys()
    }
}

/// A serde helper to store a boolean as an object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileConfigSrc {
    Path(PathBuf),
    Obj(FileConfigSource),
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfigSource {
    pub path: PathBuf,
    /// Zoom-level bounds for tile caching.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    pub cache: CachePolicy,
}

#[cfg(feature = "_tiles")]
pub async fn resolve_files<T: TileSourceConfiguration>(
    config: &mut FileConfigEnum<T>,
    idr: &IdResolver,
    extension: &[&str],
    default_cache: CachePolicy,
) -> MartinResult<(Vec<BoxedSource>, Vec<TileSourceWarning>)> {
    resolve_int(config, idr, extension, default_cache).await
}

#[cfg(feature = "_tiles")]
async fn resolve_int<T: TileSourceConfiguration>(
    config: &mut FileConfigEnum<T>,
    idr: &IdResolver,
    extension: &[&str],
    default_cache: CachePolicy,
) -> MartinResult<(Vec<BoxedSource>, Vec<TileSourceWarning>)> {
    let Some(cfg) = config.extract_file_config() else {
        return Ok((vec![], vec![]));
    };

    let mut results = Vec::new();
    let mut warnings = Vec::new();
    let mut configs = BTreeMap::new();
    let mut files = HashSet::new();
    let mut directories = Vec::new();

    if let Some(sources) = cfg.sources {
        for (id, source) in sources {
            match resolve_one_source_int(
                &cfg.custom,
                idr,
                &id,
                source,
                &mut files,
                &mut configs,
                default_cache,
            )
            .await
            {
                Ok(src) => results.push(src),
                Err(err) => {
                    warnings.push(TileSourceWarning::SourceError {
                        source_id: id,
                        error: err.to_string(),
                    });
                }
            }
        }
    }

    for path in cfg.paths {
        match resolve_one_path_int(
            &cfg.custom,
            idr,
            extension,
            path.clone(),
            &mut files,
            &mut directories,
            &mut configs,
            default_cache,
        )
        .await
        {
            Ok(sources) => results.extend(sources),
            Err(err) => {
                warnings.push(TileSourceWarning::PathError {
                    path: path.display().to_string(),
                    error: err.to_string(),
                });
            }
        }
    }

    *config = FileConfigEnum::new_extended(directories, configs, cfg.custom);

    Ok((results, warnings))
}

/// Resolves a single tile source configuration and returns a boxed source for further processing.
///
/// This function processes a tile source configuration using a given custom implementation of
/// `TileSourceConfiguration` and resolves its ID using `IdResolver`.
/// It determines if the source is a URL or a file path, configures the source accordingly.
#[cfg(feature = "_tiles")]
async fn resolve_one_source_int<T: TileSourceConfiguration>(
    custom: &T,
    idr: &IdResolver,
    id: &str,
    source: FileConfigSrc,
    files: &mut HashSet<PathBuf>,
    configs: &mut BTreeMap<String, FileConfigSrc>,
    default_cache: CachePolicy,
) -> MartinResult<BoxedSource> {
    let cache = source.cache_zoom().or(default_cache);
    let result;
    if let Some(url) = parse_url(T::parse_urls(), source.get_path())? {
        let dup = !files.insert(source.get_path().clone());
        let dup = if dup { "duplicate " } else { "" };
        let id = idr.resolve(id, url.to_string());
        configs.insert(id.clone(), source);
        result = custom
            .new_sources_url(id.clone(), url.clone(), cache)
            .await?;
        info!("Configured {dup}source {id} from {}", sanitize_url(&url));
    } else {
        let can = source.abs_path()?;
        let dup = !files.insert(can.clone());
        let dup = if dup { "duplicate " } else { "" };
        let id = idr.resolve(id, can.to_string_lossy().to_string());
        info!("Configured {dup}source {id} from {}", can.display());
        configs.insert(id.clone(), source.clone());
        result = custom.new_sources(id, source.into_path(), cache).await?;
    }
    Ok(result)
}

/// Resolves a single path, configuring sources based on the given tile source configuration.
///
/// This function processes a given `PathBuf`, checking if it represents a file, directory,
/// or a URL, and then it performs the necessary steps to configure tile sources.
#[cfg(feature = "_tiles")]
#[expect(clippy::too_many_arguments)]
async fn resolve_one_path_int<T: TileSourceConfiguration>(
    custom: &T,
    idr: &IdResolver,
    extension: &[&str],
    path: PathBuf,
    files: &mut HashSet<PathBuf>,
    directories: &mut Vec<PathBuf>,
    configs: &mut BTreeMap<String, FileConfigSrc>,
    default_cache: CachePolicy,
) -> MartinResult<Vec<BoxedSource>> {
    let mut results = Vec::new();

    if let Some(url) = parse_url(T::parse_urls(), &path)? {
        let target_ext = extension.iter().find(|&e| url.to_string().ends_with(e));
        let id = if let Some(ext) = target_ext {
            url.path_segments()
                .and_then(Iterator::last)
                .and_then(|s| {
                    // Strip extension and trailing dot, or keep the original string
                    s.strip_suffix(ext)
                        .and_then(|s| s.strip_suffix('.'))
                        .or(Some(s))
                })
                .unwrap_or("web_source")
        } else {
            "web_source"
        };

        let id = idr.resolve(id, url.to_string());
        configs.insert(id.clone(), FileConfigSrc::Path(path));
        results.push(
            custom
                .new_sources_url(id.clone(), url.clone(), default_cache)
                .await?,
        );
        info!("Configured source {id} from URL {}", sanitize_url(&url));
    } else {
        let is_dir = path.is_dir();
        let dir_files = if is_dir {
            // directories will be kept in the config just in case there are new files
            directories.push(path.clone());
            collect_files_with_extension(&path, extension)?
        } else if path.is_file() {
            vec![path]
        } else {
            return Err(MartinError::from(ConfigFileError::InvalidFilePath(
                path.canonicalize().unwrap_or(path),
            )));
        };
        for path in dir_files {
            let can = path
                .canonicalize()
                .map_err(|e| ConfigFileError::IoError(e, path.clone()))?;
            if files.contains(&can) {
                if !is_dir {
                    warn!("Ignoring duplicate MBTiles path: {}", can.display());
                }
                continue;
            }
            let id = path.file_stem().map_or_else(
                || "_unknown".to_string(),
                |s| s.to_string_lossy().to_string(),
            );
            let id = idr.resolve(&id, can.to_string_lossy().to_string());
            info!("Configured source {id} from {}", can.display());
            files.insert(can);
            configs.insert(id.clone(), FileConfigSrc::Path(path.clone()));
            results.push(custom.new_sources(id, path, default_cache).await?);
        }
    }
    Ok(results)
}

/// Returns a vector of file paths matching any `allowed_extension` within the given directory.
///
/// # Errors
///
/// Returns an error if Rust's underlying [`read_dir`](std::fs::read_dir) returns an error.
#[cfg(feature = "_tiles")]
fn collect_files_with_extension(
    base_path: &Path,
    allowed_extension: &[&str],
) -> Result<Vec<PathBuf>, ConfigFileError> {
    Ok(base_path
        .read_dir()
        .map_err(|e| ConfigFileError::IoError(e, base_path.to_path_buf()))?
        .filter_map(Result::ok)
        .filter(|f| {
            f.path().extension().is_some_and(|actual_ext| {
                allowed_extension
                    .iter()
                    .any(|expected_ext| *expected_ext == actual_ext)
            }) && f.path().is_file()
        })
        .map(|f| f.path())
        .collect())
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

#[cfg(feature = "_tiles")]
fn parse_url(is_enabled: bool, path: &Path) -> Result<Option<Url>, ConfigFileError> {
    if !is_enabled {
        return Ok(None);
    }
    let url_schemes = [
        "s3://", "s3a://", "gs://", "az://", "adl://", "azure://", "abfs://", "abfss://",
        "http://", "https://", "file://",
    ];
    path.to_str()
        .filter(|v| url_schemes.iter().any(|scheme| v.starts_with(scheme)))
        .map(|v| Url::parse(v).map_err(|e| ConfigFileError::InvalidSourceUrl(e, v.to_string())))
        .transpose()
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct CachePolicy {
    #[serde(flatten)]
    zoom: CacheZoomRange,
}

impl CachePolicy {
    /// Creates a new `CachePolicy` with the given zoom range.
    #[must_use]
    pub fn new(zoom: CacheZoomRange) -> Self {
        Self { zoom }
    }

    /// Creates a disabled `CachePolicy` where caching is turned off.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            zoom: CacheZoomRange::disabled(),
        }
    }

    /// Returns the zoom-level bounds for caching.
    #[must_use]
    pub fn zoom(self) -> CacheZoomRange {
        self.zoom
    }

    /// Returns `true` if no cache bounds are configured.
    #[must_use]
    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde skip_serializing_if requires &self"
    )]
    pub fn is_empty(&self) -> bool {
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
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum CachePolicyHelper {
            String(String),
            Struct {
                #[serde(flatten, default)]
                zoom: CacheZoomRange,
            },
        }

        match CachePolicyHelper::deserialize(deserializer)? {
            CachePolicyHelper::String(s) if s == "disable" => Ok(Self::disabled()),
            CachePolicyHelper::String(s) => Err(serde::de::Error::custom(format!(
                "invalid cache policy string: {s:?}, expected \"disable\""
            ))),
            CachePolicyHelper::Struct { zoom } => Ok(Self { zoom }),
        }
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct GlobalCacheConfig {
    /// Total cache budget in megabytes (0 to disable).
    /// Split across tile, pmtiles, sprite, and font caches by default.
    pub size_mb: Option<u64>,
    /// Tile cache size override in megabytes.
    /// Defaults to `size_mb / 2`.
    pub tile_size_mb: Option<u64>,
    /// Maximum lifetime for all cache entries (time-to-live from creation).
    /// Supports human-readable formats: "1h", "30m", "1d".
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub expiry: Option<Duration>,
    /// Maximum idle time for all cache entries (time-to-idle since last access).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub idle_timeout: Option<Duration>,
    /// Tile-specific TTL override. Takes precedence over `expiry` for tiles.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub tile_expiry: Option<Duration>,
    /// Tile-specific idle timeout override. Takes precedence over `idle_timeout` for tiles.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub tile_idle_timeout: Option<Duration>,
    #[serde(flatten)]
    zoom: CacheZoomRange,
}

impl GlobalCacheConfig {
    /// Creates a disabled `GlobalCacheConfig` with size 0 and minzoom > maxzoom.
    #[must_use]
    pub fn disabled() -> Self {
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
    pub fn policy(self) -> CachePolicy {
        CachePolicy::new(self.zoom)
    }

    /// Returns `true` if no cache settings are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size_mb.is_none()
            && self.tile_size_mb.is_none()
            && self.expiry.is_none()
            && self.idle_timeout.is_none()
            && self.tile_expiry.is_none()
            && self.tile_idle_timeout.is_none()
            && self.zoom.is_empty()
    }
}

impl<'de> Deserialize<'de> for GlobalCacheConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            String(String),
            Struct {
                size_mb: Option<u64>,
                tile_size_mb: Option<u64>,
                #[serde(
                    default,
                    skip_serializing_if = "Option::is_none",
                    with = "humantime_serde"
                )]
                expiry: Option<Duration>,
                #[serde(
                    default,
                    skip_serializing_if = "Option::is_none",
                    with = "humantime_serde"
                )]
                idle_timeout: Option<Duration>,
                #[serde(
                    default,
                    skip_serializing_if = "Option::is_none",
                    with = "humantime_serde"
                )]
                tile_expiry: Option<Duration>,
                #[serde(
                    default,
                    skip_serializing_if = "Option::is_none",
                    with = "humantime_serde"
                )]
                tile_idle_timeout: Option<Duration>,
                #[serde(flatten, default)]
                zoom: CacheZoomRange,
            },
        }

        match Helper::deserialize(deserializer)? {
            Helper::String(s) if s == "disable" => Ok(Self::disabled()),
            Helper::String(s) => Err(serde::de::Error::custom(format!(
                "invalid cache config string: {s:?}, expected \"disable\""
            ))),
            Helper::Struct {
                size_mb,
                tile_size_mb,
                expiry,
                idle_timeout,
                tile_expiry,
                tile_idle_timeout,
                zoom,
            } => Ok(Self {
                size_mb,
                tile_size_mb,
                expiry,
                idle_timeout,
                tile_expiry,
                tile_idle_timeout,
                zoom,
            }),
        }
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
pub struct CacheSizeConfig {
    /// Cache size in megabytes for this source type (0 to disable).
    pub size_mb: Option<u64>,
    /// Maximum lifetime of cache entries (time-to-live from creation).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub expiry: Option<Duration>,
    /// Maximum idle time before cache entries are evicted (time-to-idle since last access).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub idle_timeout: Option<Duration>,
}

impl CacheSizeConfig {
    /// Returns `true` if no cache settings are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.size_mb.is_none() && self.expiry.is_none() && self.idle_timeout.is_none()
    }
}

impl<'de> Deserialize<'de> for CacheSizeConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            String(String),
            Struct {
                size_mb: Option<u64>,
                #[serde(
                    default,
                    skip_serializing_if = "Option::is_none",
                    with = "humantime_serde"
                )]
                expiry: Option<Duration>,
                #[serde(
                    default,
                    skip_serializing_if = "Option::is_none",
                    with = "humantime_serde"
                )]
                idle_timeout: Option<Duration>,
            },
        }

        match Helper::deserialize(deserializer)? {
            Helper::String(s) if s == "disable" => Ok(Self {
                size_mb: Some(0),
                expiry: None,
                idle_timeout: None,
            }),
            Helper::String(s) => Err(serde::de::Error::custom(format!(
                "invalid cache config string: {s:?}, expected \"disable\""
            ))),
            Helper::Struct {
                size_mb,
                expiry,
                idle_timeout,
            } => Ok(Self {
                size_mb,
                expiry,
                idle_timeout,
            }),
        }
    }
}

pub type UnrecognizedValues = HashMap<String, serde_yaml::Value>;
pub type UnrecognizedKeys = HashSet<String>;

pub fn copy_unrecognized_keys_from_config(
    result: &mut UnrecognizedKeys,
    prefix: &str,
    unrecognized: &UnrecognizedValues,
) {
    result.extend(unrecognized.keys().map(|k| format!("{prefix}{k}")));
}

#[cfg(all(test, feature = "mbtiles"))]
mod mbtiles_tests {
    use super::*;
    use crate::config::file::tiles::mbtiles::MbtConfig;
    use crate::config::primitives::IdResolver;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_invalid_path_warns_instead_of_failing() {
        let invalid_path = PathBuf::from("/nonexistent/path/");
        let invalid_source = PathBuf::from("/nonexistent/path/to/file.mbtiles");
        let mut file_sources = BTreeMap::new();
        file_sources.insert(
            "test_source".to_string(),
            FileConfigSrc::Path(invalid_source.clone()),
        );
        let mut config = FileConfigEnum::<MbtConfig>::Config(FileConfig {
            paths: OptOneMany::One(invalid_path.clone()),
            sources: Some(file_sources),
            custom: MbtConfig::default(),
        });

        let idr = IdResolver::new(&[]);
        let result = resolve_files(&mut config, &idr, &["mbtiles"], CachePolicy::default()).await;

        let (sources, warnings) = result.unwrap();
        assert_eq!(sources.len(), 0);
        assert_eq!(warnings.len(), 2);
    }
}

#[cfg(all(test, feature = "pmtiles"))]
mod pmtiles_tests {
    use super::*;
    use crate::config::file::tiles::pmtiles::PmtConfig;
    use crate::config::primitives::IdResolver;

    #[tokio::test]
    #[tracing_test::traced_test]
    async fn test_invalid_path_warns_instead_of_failing() {
        let invalid_path = PathBuf::from("/nonexistent/path/");
        let invalid_source = PathBuf::from("/nonexistent/path/to/file.pmtiles");
        let mut file_sources = BTreeMap::new();
        file_sources.insert(
            "test_source".to_string(),
            FileConfigSrc::Path(invalid_source.clone()),
        );
        let mut config = FileConfigEnum::<PmtConfig>::Config(FileConfig {
            paths: OptOneMany::One(invalid_path.clone()),
            sources: Some(file_sources),
            custom: PmtConfig::default(),
        });

        let idr = IdResolver::new(&[]);
        let result = resolve_files(&mut config, &idr, &["pmtiles"], CachePolicy::default()).await;

        let (sources, warnings) = result.unwrap();
        assert_eq!(sources.len(), 0);
        assert_eq!(warnings.len(), 2);
    }
}
