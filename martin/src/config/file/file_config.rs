use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Debug};
use std::marker::PhantomData;
use std::mem;
#[cfg(feature = "_tiles")]
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use martin_core::CacheZoomRange;
#[cfg(feature = "_tiles")]
use martin_core::tiles::BoxedSource;
use serde::de::value::{MapAccessDeserializer, SeqAccessDeserializer};
use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
#[cfg(feature = "_tiles")]
use tracing::{info, warn};
#[cfg(feature = "_tiles")]
use url::Url;

#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::MltProcessConfig;
use crate::config::file::{ConfigFileError, ConfigFileResult};
#[cfg(feature = "_tiles")]
use crate::config::file::{ResolutionResult, TileSourceWarning};
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

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum FileConfigEnum<T> {
    #[default]
    None,
    Path(PathBuf),
    Paths(Vec<PathBuf>),
    Config(FileConfig<T>),
}

impl<'de, T> Deserialize<'de> for FileConfigEnum<T>
where
    T: Deserialize<'de> + Default,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FileConfigEnumVisitor<T>(PhantomData<T>);

        impl<'de, T> Visitor<'de> for FileConfigEnumVisitor<T>
        where
            T: Deserialize<'de> + Default,
        {
            type Value = FileConfigEnum<T>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(
                    "a path string, a list of path strings, or a configuration map with \
                     `paths` and/or `sources`",
                )
            }

            fn visit_unit<E: de::Error>(self) -> Result<FileConfigEnum<T>, E> {
                Ok(FileConfigEnum::None)
            }

            fn visit_none<E: de::Error>(self) -> Result<FileConfigEnum<T>, E> {
                Ok(FileConfigEnum::None)
            }

            fn visit_str<E: de::Error>(self, value: &str) -> Result<FileConfigEnum<T>, E> {
                Ok(FileConfigEnum::Path(PathBuf::from(value)))
            }

            fn visit_string<E: de::Error>(self, value: String) -> Result<FileConfigEnum<T>, E> {
                Ok(FileConfigEnum::Path(PathBuf::from(value)))
            }

            fn visit_seq<S: SeqAccess<'de>>(self, seq: S) -> Result<FileConfigEnum<T>, S::Error> {
                let paths: Vec<PathBuf> =
                    Deserialize::deserialize(SeqAccessDeserializer::new(seq))?;
                Ok(FileConfigEnum::Paths(paths))
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<FileConfigEnum<T>, M::Error> {
                let cfg = FileConfig::<T>::deserialize(MapAccessDeserializer::new(map))?;
                Ok(FileConfigEnum::Config(cfg))
            }

            // Numbers / booleans fall through to serde's default `invalid_type` path,
            // which is what attaches the source span via saphyr's deserializer.
        }

        deserializer.deserialize_any(FileConfigEnumVisitor(PhantomData))
    }
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
        if configs.is_empty() {
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
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
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
#[derive(Clone, Debug, PartialEq, Serialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum FileConfigSrc {
    Path(PathBuf),
    Obj(FileConfigSource),
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
                Ok(FileConfigSrc::Obj(obj))
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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct FileConfigSource {
    pub path: PathBuf,
    /// MVT→MLT encoder settings for this source.
    /// Overrides source-type and global `convert-to-mlt`.
    #[cfg(all(feature = "mlt", feature = "_tiles"))]
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "convert-to-mlt"
    )]
    pub convert_to_mlt: Option<MltProcessConfig>,
    /// Zoom-level bounds for tile caching.
    #[serde(default, skip_serializing_if = "CachePolicy::is_empty")]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "CachePolicyShape"))]
    pub cache: CachePolicy,
}

#[cfg(feature = "_tiles")]
pub async fn resolve_files<T: TileSourceConfiguration>(
    config: &mut FileConfigEnum<T>,
    idr: &IdResolver,
    extension: &[&str],
    default_cache: CachePolicy,
) -> ResolutionResult {
    resolve_int(config, idr, extension, default_cache).await
}

#[cfg(feature = "_tiles")]
async fn resolve_int<T: TileSourceConfiguration>(
    config: &mut FileConfigEnum<T>,
    idr: &IdResolver,
    extension: &[&str],
    default_cache: CachePolicy,
) -> ResolutionResult {
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
            Ok((sources, path_warnings)) => {
                results.extend(sources);
                warnings.extend(path_warnings);
            }
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
) -> ResolutionResult {
    let mut results = Vec::new();
    let mut warnings = Vec::new();

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
            // Only commit `id`/`can` to bookkeeping after a successful init so a single
            // bad file inside a directory does not poison the whole batch — without this,
            // `on_invalid: warn` would still drop every sibling source.
            match custom
                .new_sources(id.clone(), path.clone(), default_cache)
                .await
            {
                Ok(src) => {
                    info!("Configured source {id} from {}", can.display());
                    files.insert(can);
                    configs.insert(id, FileConfigSrc::Path(path));
                    results.push(src);
                }
                Err(err) => {
                    warnings.push(TileSourceWarning::PathError {
                        path: path.display().to_string(),
                        error: err.to_string(),
                    });
                }
            }
        }
    }
    Ok((results, warnings))
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub expiry: Option<Duration>,
    /// Maximum idle time for all cache entries (time-to-idle since last access).
    /// Entries are evicted if not accessed within this duration.
    /// default: null (no idle timeout)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub idle_timeout: Option<Duration>,
    /// Tile-specific TTL override. Takes precedence over `cache.expiry` for tiles.
    /// default: null (inherits from `cache.expiry`)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub tile_expiry: Option<Duration>,
    /// Tile-specific idle timeout override. Takes precedence over `cache.idle_timeout` for tiles.
    /// default: null (inherits from `cache.idle_timeout`)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
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
        // Inner struct that handles the map case via the derive — we still get good error
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CacheSizeConfig {
    /// Size of the cache in MB (0 to disable).
    /// default: inherits from `cache.size_mb` (with a per-source split)
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &64u64))]
    pub size_mb: Option<u64>,
    /// Maximum lifetime for cache entries.
    /// default: null (inherits from `cache.expiry`)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub expiry: Option<Duration>,
    /// Maximum idle time for cache entries.
    /// default: null (inherits from `cache.idle_timeout`)
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "Option<String>"))]
    pub idle_timeout: Option<Duration>,
}

impl CacheSizeConfig {
    /// Returns `true` if no cache settings are configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
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

pub type UnrecognizedValues = HashMap<String, serde_yaml::Value>;
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

    /// Inner config used to instantiate `FileConfigEnum<T>` / `FileConfig<T>` in success-path
    /// tests without depending on a real source-type config.
    #[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
    struct TestCustom {
        #[serde(default)]
        flag: bool,
    }

    impl ConfigurationLivecycleHooks for TestCustom {
        fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
            UnrecognizedKeys::new()
        }
    }

    // Failure-path tests run through the full `parse_config` pipeline using realistic
    // `Config` fields (e.g. `pmtiles:` for `FileConfigEnum`, `mbtiles.sources` for
    // `FileConfigSrc`, `cache:` for the cache deserializers) so each snapshot mirrors what
    // the user sees on the command line.

    // ----- FileConfigEnum<T> -----

    #[test]
    fn file_config_enum_null_is_none() {
        let cfg = parse_yaml::<FileConfigEnum<TestCustom>>("null");
        assert_eq!(cfg, FileConfigEnum::None);
    }

    #[test]
    fn file_config_enum_string_is_path() {
        let cfg = parse_yaml::<FileConfigEnum<TestCustom>>("/tmp/tiles");
        assert_eq!(cfg, FileConfigEnum::Path(PathBuf::from("/tmp/tiles")));
    }

    #[test]
    fn file_config_enum_seq_is_paths() {
        let cfg = parse_yaml::<FileConfigEnum<TestCustom>>("[/a, /b]");
        assert_eq!(
            cfg,
            FileConfigEnum::Paths(vec![PathBuf::from("/a"), PathBuf::from("/b")])
        );
    }

    #[test]
    fn file_config_enum_map_is_config() {
        let cfg = parse_yaml::<FileConfigEnum<TestCustom>>("{ paths: [/a], flag: true }");
        let FileConfigEnum::Config(file_config) = cfg else {
            panic!("expected Config variant");
        };
        assert_eq!(file_config.paths, OptOneMany::One(PathBuf::from("/a")));
        assert!(file_config.custom.flag);
    }

    #[test]
    #[cfg(feature = "pmtiles")]
    fn file_config_enum_rejects_integer() {
        insta::assert_snapshot!(render_failure("pmtiles: 42\n"), @"
         × invalid type: integer `42`, expected a path string, a list of path
         │ strings, or a configuration map with `paths` and/or `sources`
          ╭─[config.yaml:1:1]
        1 │ pmtiles: 42
          · ───┬───
          ·    ╰── invalid type: integer `42`, expected a path string, a list of path strings, or a configuration map with `paths` and/or `sources`
          ╰────
        ");
    }

    #[test]
    #[cfg(feature = "pmtiles")]
    fn file_config_enum_rejects_bool() {
        insta::assert_snapshot!(render_failure("pmtiles: true\n"), @"
         × invalid type: boolean `true`, expected a path string, a list of path
         │ strings, or a configuration map with `paths` and/or `sources`
          ╭─[config.yaml:1:1]
        1 │ pmtiles: true
          · ───┬───
          ·    ╰── invalid type: boolean `true`, expected a path string, a list of path strings, or a configuration map with `paths` and/or `sources`
          ╰────
        ");
    }

    #[test]
    #[cfg(feature = "pmtiles")]
    fn file_config_enum_path_list_with_nested_map_fails() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                pmtiles:
                  paths:
                    - { not_a_path: true }
            "}),
            @"
         × unexpected event: expected string scalar
          ╭─[config.yaml:3:7]
        2 │   paths:
        3 │     - { not_a_path: true }
          ·       ─┬
          ·        ╰── unexpected event: expected string scalar
          ╰────
        "
        );
    }

    // ----- FileConfigSrc -----

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
         × invalid type: integer `5`, expected a path string or a configuration map
         │ with a `path` field
          ╭─[config.yaml:3:5]
        2 │   sources:
        3 │     foo: 5
          ·     ─┬─
          ·      ╰── invalid type: integer `5`, expected a path string or a configuration map with a `path` field
          ╰────
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
         × invalid type: boolean `true`, expected a path string or a configuration
         │ map with a `path` field
          ╭─[config.yaml:3:5]
        2 │   sources:
        3 │     foo: true
          ·     ─┬─
          ·      ╰── invalid type: boolean `true`, expected a path string or a configuration map with a `path` field
          ╰────
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
         × invalid type: sequence, expected a path string or a configuration map with
         │ a `path` field
          ╭─[config.yaml:3:5]
        2 │   sources:
        3 │     foo: [a, b]
          ·     ─┬─
          ·      ╰── invalid type: sequence, expected a path string or a configuration map with a `path` field
          ╰────
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
         × invalid cache config string "enable"; the only accepted string form is
         │ `disable`
          ╭─[config.yaml:1:8]
        1 │ cache: enable
          ·        ───┬──
          ·           ╰── invalid cache config string "enable"; the only accepted string form is `disable`
          ╰────
        "#);
    }

    #[test]
    fn global_cache_rejects_integer() {
        insta::assert_snapshot!(render_failure("cache: 42\n"), @"
         × invalid type: integer `42`, expected either the literal `disable` or a
         │ cache configuration map (e.g. `{ size_mb: 512, tile_size_mb: 256 }`)
          ╭─[config.yaml:1:1]
        1 │ cache: 42
          · ──┬──
          ·   ╰── invalid type: integer `42`, expected either the literal `disable` or a cache configuration map (e.g. `{ size_mb: 512, tile_size_mb: 256 }`)
          ╰────
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
    #[cfg(feature = "sprites")]
    fn cache_size_rejects_other_string() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                sprites:
                  cache: yes
            "}),
            @"
         × invalid type: boolean `true`, expected either the literal `disable` or a
         │ cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)
          ╭─[config.yaml:2:3]
        1 │ sprites:
        2 │   cache: yes
          ·   ──┬──
          ·     ╰── invalid type: boolean `true`, expected either the literal `disable` or a cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)
          ╰────
        "
        );
    }

    #[test]
    #[cfg(feature = "sprites")]
    fn cache_size_rejects_integer() {
        insta::assert_snapshot!(
            render_failure(indoc::indoc! {"
                sprites:
                  cache: 42
            "}),
            @"
         × invalid type: integer `42`, expected either the literal `disable` or a
         │ cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)
          ╭─[config.yaml:2:3]
        1 │ sprites:
        2 │   cache: 42
          ·   ──┬──
          ·     ╰── invalid type: integer `42`, expected either the literal `disable` or a cache configuration map (e.g. `{ size_mb: 64, expiry: 1h }`)
          ╰────
        "
        );
    }

    // ----- CachePolicy (constructed internally, not surfaced as a config-tree field) -----
    //
    // `CachePolicy` is built from `CacheZoomRange` derived from per-source defaults; it is
    // not addressable via a top-level YAML path. We exercise the deserializer directly here
    // and rely on the `cache:` and per-source `cache:` block tests above to cover the
    // user-visible diagnostic surface.

    #[test]
    fn cache_policy_disable_string() {
        let cfg = parse_yaml::<CachePolicy>("disable");
        assert_eq!(cfg, CachePolicy::disabled());
    }

    #[test]
    fn cache_policy_map() {
        let cfg = parse_yaml::<CachePolicy>("{ minzoom: 0, maxzoom: 14 }");
        let dumped = serde_yaml::to_string(&cfg).unwrap();
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

/// Folder-source path resolution: a single bad file in a directory must not
/// drop its valid siblings. Regression for
/// <https://github.com/maplibre/martin/discussions/2767>.
#[cfg(all(test, feature = "_tiles"))]
mod folder_source_tests {
    use async_trait::async_trait;
    use insta::assert_yaml_snapshot;
    use martin_core::CacheZoomRange;
    use martin_core::tiles::{MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tempfile::TempDir;
    use tilejson::{TileJSON, tilejson};

    use super::*;
    use crate::MartinError;
    use crate::config::primitives::IdResolver;

    /// Files whose stem starts with this prefix are treated as invalid by [`FakeConfig`].
    const BAD_PREFIX: &str = "bad_";

    #[derive(Clone, Debug, Default, PartialEq)]
    struct FakeConfig;

    impl ConfigurationLivecycleHooks for FakeConfig {
        fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
            UnrecognizedKeys::new()
        }
    }

    impl TileSourceConfiguration for FakeConfig {
        fn parse_urls() -> bool {
            false
        }
        async fn new_sources(
            &self,
            id: String,
            path: PathBuf,
            _cache: CachePolicy,
        ) -> MartinResult<BoxedSource> {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem.starts_with(BAD_PREFIX) {
                Err(MartinError::from(ConfigFileError::InvalidFilePath(path)))
            } else {
                Ok(Box::new(FakeSource {
                    id,
                    tj: tilejson! { tiles: vec![] },
                }))
            }
        }
        async fn new_sources_url(
            &self,
            _id: String,
            _url: Url,
            _cache: CachePolicy,
        ) -> MartinResult<BoxedSource> {
            unreachable!()
        }
    }

    #[derive(Debug, Clone)]
    struct FakeSource {
        id: String,
        tj: TileJSON,
    }

    #[async_trait]
    impl Source for FakeSource {
        fn get_id(&self) -> &str {
            &self.id
        }
        fn get_tilejson(&self) -> &TileJSON {
            &self.tj
        }
        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Mvt, Encoding::Uncompressed)
        }
        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
        }
        fn cache_zoom(&self) -> CacheZoomRange {
            CacheZoomRange::default()
        }
        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            Ok(vec![])
        }
    }

    /// Resolves a freshly-created tempdir populated with `good` good files and
    /// `bad` bad files, returning sorted source ids + warning strings with the
    /// random tempdir prefix replaced by `<DIR>` for snapshot stability.
    async fn resolve_mixed_dir(good: usize, bad: usize) -> (Vec<String>, Vec<String>) {
        let dir = TempDir::new().expect("create tempdir");
        for i in 0..good {
            std::fs::write(dir.path().join(format!("good_{i}.tiles")), b"").expect("write good");
        }
        for i in 0..bad {
            std::fs::write(dir.path().join(format!("{BAD_PREFIX}{i}.tiles")), b"")
                .expect("write bad");
        }

        let mut config = FileConfigEnum::<FakeConfig>::Path(dir.path().to_path_buf());
        let idr = IdResolver::new(&[]);
        let (sources, warnings) =
            resolve_files(&mut config, &idr, &["tiles"], CachePolicy::default())
                .await
                .expect("resolve_files always returns Ok; OnInvalid decides fatality");

        let prefix = dir.path().to_string_lossy().to_string();
        let mut ids: Vec<String> = sources.iter().map(|s| s.get_id().to_string()).collect();
        ids.sort();
        let mut msgs: Vec<String> = warnings
            .iter()
            .map(|w| w.to_string().replace(&prefix, "<DIR>"))
            .collect();
        msgs.sort();
        (ids, msgs)
    }

    #[tokio::test]
    async fn one_good_one_bad() {
        let (sources, warnings) = resolve_mixed_dir(1, 1).await;
        assert_yaml_snapshot!(sources, @"- good_0");
        assert_yaml_snapshot!(warnings, @r#"- "Path <DIR>/bad_0.tiles: Source path is not a file: <DIR>/bad_0.tiles""#);
    }

    #[tokio::test]
    async fn two_good_two_bad() {
        let (sources, warnings) = resolve_mixed_dir(2, 2).await;
        assert_yaml_snapshot!(sources, @r"
        - good_0
        - good_1
        ");
        assert_yaml_snapshot!(warnings, @r#"
        - "Path <DIR>/bad_0.tiles: Source path is not a file: <DIR>/bad_0.tiles"
        - "Path <DIR>/bad_1.tiles: Source path is not a file: <DIR>/bad_1.tiles"
        "#);
    }

    #[tokio::test]
    async fn all_bad() {
        let (sources, warnings) = resolve_mixed_dir(0, 2).await;
        assert_yaml_snapshot!(sources, @"[]");
        assert_yaml_snapshot!(warnings, @r#"
        - "Path <DIR>/bad_0.tiles: Source path is not a file: <DIR>/bad_0.tiles"
        - "Path <DIR>/bad_1.tiles: Source path is not a file: <DIR>/bad_1.tiles"
        "#);
    }

    #[tokio::test]
    async fn all_good() {
        let (sources, warnings) = resolve_mixed_dir(2, 0).await;
        assert_yaml_snapshot!(sources, @r"
        - good_0
        - good_1
        ");
        assert_yaml_snapshot!(warnings, @"[]");
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
