use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::mem;
#[cfg(feature = "_tiles")]
use std::path::Path;
use std::path::PathBuf;

#[cfg(feature = "_tiles")]
use log::{info, warn};
#[cfg(feature = "_tiles")]
use martin_core::config::IdResolver;
use martin_core::config::OptOneMany;
#[cfg(feature = "_tiles")]
use martin_core::tiles::BoxedSource;
use serde::{Deserialize, Serialize};
#[cfg(feature = "_tiles")]
use url::Url;

#[cfg(feature = "_tiles")]
use crate::config::file::TileSourceWarning;
use crate::config::file::{ConfigFileError, ConfigFileResult};
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
    fn new_sources(
        &self,
        id: String,
        path: PathBuf,
    ) -> impl Future<Output = MartinResult<BoxedSource>> + Send;

    /// Asynchronously creates a new `BoxedSource` from a **remote** `url` using the given `id`.
    ///
    /// This function is called for each discovered source path that is a valid URL.
    fn new_sources_url(
        &self,
        id: String,
        url: Url,
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
    pub fn new(paths: Vec<PathBuf>) -> FileConfigEnum<T> {
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
                0 => FileConfigEnum::None,
                1 => FileConfigEnum::Path(paths.into_iter().next().expect("one path exists")),
                _ => FileConfigEnum::Paths(paths),
            }
        } else {
            FileConfigEnum::Config(FileConfig {
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
            FileConfigEnum::None => None,
            FileConfigEnum::Path(path) => Some(FileConfig {
                paths: OptOneMany::One(mem::take(path)),
                ..FileConfig::default()
            }),
            FileConfigEnum::Paths(paths) => Some(FileConfig {
                paths: OptOneMany::Many(mem::take(paths)),
                ..Default::default()
            }),
            FileConfigEnum::Config(cfg) => Some(mem::take(cfg)),
        }
    }

    /// convert path/paths and the config enums
    #[must_use]
    pub fn into_config(self) -> FileConfigEnum<T> {
        match self {
            FileConfigEnum::Path(path) => FileConfigEnum::Config(FileConfig {
                paths: OptOneMany::One(path),
                sources: None,
                custom: T::default(),
            }),
            FileConfigEnum::Paths(paths) => FileConfigEnum::Config(FileConfig {
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
}

#[cfg(feature = "_tiles")]
pub async fn resolve_files<T: TileSourceConfiguration>(
    config: &mut FileConfigEnum<T>,
    idr: &IdResolver,
    extension: &[&str],
) -> MartinResult<(Vec<BoxedSource>, Vec<TileSourceWarning>)> {
    resolve_int(config, idr, extension).await
}

#[cfg(feature = "_tiles")]
async fn resolve_int<T: TileSourceConfiguration>(
    config: &mut FileConfigEnum<T>,
    idr: &IdResolver,
    extension: &[&str],
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
            match resolve_one_source_int(&cfg.custom, idr, &id, source, &mut files, &mut configs)
                .await
            {
                Ok(src) => results.push(src),
                Err(err) => {
                    warnings.push(TileSourceWarning::SourceError {
                        source_id: id,
                        error: err,
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
        )
        .await
        {
            Ok(sources) => results.extend(sources),
            Err(err) => {
                warnings.push(TileSourceWarning::PathError {
                    path: path.display().to_string(),
                    error: err,
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
) -> MartinResult<BoxedSource> {
    let result;
    if let Some(url) = parse_url(T::parse_urls(), source.get_path())? {
        let dup = !files.insert(source.get_path().clone());
        let dup = if dup { "duplicate " } else { "" };
        let id = idr.resolve(id, url.to_string());
        configs.insert(id.clone(), source);
        result = custom.new_sources_url(id.clone(), url.clone()).await?;
        info!("Configured {dup}source {id} from {}", sanitize_url(&url));
    } else {
        let can = source.abs_path()?;
        let dup = !files.insert(can.clone());
        let dup = if dup { "duplicate " } else { "" };
        let id = idr.resolve(id, can.to_string_lossy().to_string());
        info!("Configured {dup}source {id} from {}", can.display());
        configs.insert(id.clone(), source.clone());
        result = custom.new_sources(id, source.into_path()).await?;
    }
    Ok(result)
}

/// Resolves a single path, configuring sources based on the given tile source configuration.
///
/// This function processes a given `PathBuf`, checking if it represents a file, directory,
/// or a URL, and then it performs the necessary steps to configure tile sources.
#[cfg(feature = "_tiles")]
async fn resolve_one_path_int<T: TileSourceConfiguration>(
    custom: &T,
    idr: &IdResolver,
    extension: &[&str],
    path: PathBuf,
    files: &mut HashSet<PathBuf>,
    directories: &mut Vec<PathBuf>,
    configs: &mut BTreeMap<String, FileConfigSrc>,
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
        results.push(custom.new_sources_url(id.clone(), url.clone()).await?);
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
            results.push(custom.new_sources(id, path).await?);
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
            f.path()
                .extension()
                .filter(|actual_ext| {
                    allowed_extension
                        .iter()
                        .any(|expected_ext| expected_ext == actual_ext)
                })
                .is_some()
                && f.path().is_file()
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
    use martin_core::config::IdResolver;

    use super::*;
    use crate::config::file::tiles::mbtiles::MbtConfig;

    #[tokio::test]
    async fn test_invalid_path_warns_instead_of_failing() {
        let _ = env_logger::builder().is_test(true).try_init();

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
        let result = resolve_files(&mut config, &idr, &["mbtiles"]).await;

        let (sources, warnings) = result.unwrap();
        assert_eq!(sources.len(), 0);
        assert_eq!(warnings.len(), 2);
    }
}

#[cfg(all(test, feature = "pmtiles"))]
mod pmtiles_tests {
    use martin_core::config::IdResolver;

    use super::*;
    use crate::config::file::tiles::pmtiles::PmtConfig;

    #[tokio::test]
    async fn test_invalid_path_warns_instead_of_failing() {
        let _ = env_logger::builder().is_test(true).try_init();

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
        let result = resolve_files(&mut config, &idr, &["pmtiles"]).await;

        let (sources, warnings) = result.unwrap();
        assert_eq!(sources.len(), 0);
        assert_eq!(warnings.len(), 2);
    }
}
