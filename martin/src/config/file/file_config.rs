use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Debug;
use std::mem;
use std::path::{Path, PathBuf};

use log::{info, warn};
use martin_core::config::OptOneMany::{self, Many, One};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::file::ConfigFileError::{
    InvalidFilePath, InvalidSourceFilePath, InvalidSourceUrl, IoError,
};
use crate::source::{TileInfoSource, TileInfoSources};
use crate::utils::{IdResolver, OptMainCache};
use crate::{MartinError, MartinResult};

pub type ConfigFileResult<T> = Result<T, ConfigFileError>;

#[derive(thiserror::Error, Debug)]
pub enum ConfigFileError {
    #[error("IO error {0}: {1}")]
    IoError(std::io::Error, PathBuf),

    #[error("Source path is not a file: {0}")]
    InvalidFilePath(PathBuf),

    #[error("Error {0} while parsing URL {1}")]
    InvalidSourceUrl(url::ParseError, String),

    #[error("Source {0} uses bad file {1}")]
    InvalidSourceFilePath(String, PathBuf),

    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidMetadata(String, PathBuf),

    #[error("At least one 'origin' must be specified in the 'cors' configuration")]
    CorsNoOriginsConfigured,

    #[cfg(feature = "styles")]
    #[error("Walk directory error {0}: {1}")]
    DirectoryWalking(walkdir::Error, PathBuf),
}

pub trait ConfigExtras: Clone + Debug + Default + PartialEq + Send {
    fn init_parsing(&mut self, _cache: OptMainCache) -> ConfigFileResult<()> {
        Ok(())
    }

    #[must_use]
    fn is_default(&self) -> bool {
        true
    }

    fn get_unrecognized(&self) -> &UnrecognizedValues;
}

pub trait SourceConfigExtras: ConfigExtras {
    #[must_use]
    fn parse_urls() -> bool {
        false
    }

    fn new_sources(
        &self,
        id: String,
        path: PathBuf,
    ) -> impl Future<Output = MartinResult<TileInfoSource>> + Send;

    fn new_sources_with_config(
        &self,
        id: String,
        path: PathBuf,
        _config: serde_yaml::Value,
    ) -> impl Future<Output = MartinResult<TileInfoSource>> + Send {
        self.new_sources(id, path)
    }

    fn new_sources_url(
        &self,
        id: String,
        url: Url,
    ) -> impl Future<Output = MartinResult<TileInfoSource>> + Send;
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

impl<T: ConfigExtras> FileConfigEnum<T> {
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
        if configs.is_empty() && custom.is_default() {
            match paths.len() {
                0 => FileConfigEnum::None,
                1 => FileConfigEnum::Path(paths.into_iter().next().unwrap()),
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

    pub fn extract_file_config(
        &mut self,
        cache: OptMainCache,
    ) -> ConfigFileResult<Option<FileConfig<T>>> {
        let mut res = match self {
            FileConfigEnum::None => return Ok(None),
            FileConfigEnum::Path(path) => FileConfig {
                paths: One(mem::take(path)),
                ..FileConfig::default()
            },
            FileConfigEnum::Paths(paths) => FileConfig {
                paths: Many(mem::take(paths)),
                ..Default::default()
            },
            FileConfigEnum::Config(cfg) => mem::take(cfg),
        };
        res.custom.init_parsing(cache)?;
        Ok(Some(res))
    }

    pub fn finalize(&self, prefix: &str) -> UnrecognizedValues {
        let mut res = UnrecognizedValues::new();
        if let Self::Config(cfg) = self {
            copy_unrecognized_config(&mut res, prefix, cfg.get_unrecognized());
        }
        res
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

impl<T: ConfigExtras> FileConfig<T> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_none()
            && self.sources.is_none()
            && self.get_unrecognized().is_empty()
            && self.custom.is_default()
    }

    pub fn get_unrecognized(&self) -> &UnrecognizedValues {
        self.custom.get_unrecognized()
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
        path.canonicalize().map_err(|e| IoError(e, path.clone()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfigSource {
    pub path: PathBuf,
    #[serde(flatten)]
    pub config: serde_yaml::Value,
}

pub async fn resolve_files<T: SourceConfigExtras>(
    config: &mut FileConfigEnum<T>,
    idr: &IdResolver,
    cache: OptMainCache,
    extension: &[&str],
) -> MartinResult<TileInfoSources> {
    resolve_int(config, idr, cache, extension).await
}

async fn resolve_int<T: SourceConfigExtras>(
    config: &mut FileConfigEnum<T>,
    idr: &IdResolver,
    cache: OptMainCache,
    extension: &[&str],
) -> MartinResult<TileInfoSources> {
    let Some(cfg) = config.extract_file_config(cache)? else {
        return Ok(TileInfoSources::default());
    };

    let mut results = TileInfoSources::default();
    let mut configs = BTreeMap::new();
    let mut files = HashSet::new();
    let mut directories = Vec::new();

    if let Some(sources) = cfg.sources {
        for (id, source) in sources {
            if let Some(url) = parse_url(T::parse_urls(), source.get_path())? {
                let dup = !files.insert(source.get_path().clone());
                let dup = if dup { "duplicate " } else { "" };
                let id = idr.resolve(&id, url.to_string());
                configs.insert(id.clone(), source);
                results.push(cfg.custom.new_sources_url(id.clone(), url.clone()).await?);
                info!("Configured {dup}source {id} from {}", sanitize_url(&url));
            } else {
                let can = source.abs_path()?;
                if !can.is_file() {
                    // todo: maybe warn instead?
                    return Err(MartinError::ConfigFileError(InvalidSourceFilePath(
                        id.to_string(),
                        can,
                    )));
                }

                let dup = !files.insert(can.clone());
                let dup = if dup { "duplicate " } else { "" };
                let id = idr.resolve(&id, can.to_string_lossy().to_string());
                info!("Configured {dup}source {id} from {}", can.display());
                configs.insert(id.clone(), source.clone());
                let path = source.get_path().clone();
                let result = match source {
                    FileConfigSrc::Path(_) => cfg.custom.new_sources(id, path).await?,
                    FileConfigSrc::Obj(obj) => {
                        cfg.custom
                            .new_sources_with_config(id, path, obj.config.clone())
                            .await?
                    }
                };
                results.push(result);
            }
        }
    }

    for path in cfg.paths {
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
            results.push(cfg.custom.new_sources_url(id.clone(), url.clone()).await?);
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
                return Err(MartinError::from(InvalidFilePath(
                    path.canonicalize().unwrap_or(path),
                )));
            };
            for path in dir_files {
                let can = path.canonicalize().map_err(|e| IoError(e, path.clone()))?;
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
                results.push(cfg.custom.new_sources(id, path).await?);
            }
        }
    }

    *config = FileConfigEnum::new_extended(directories, configs, cfg.custom);

    Ok(results)
}

/// Returns a vector of file paths matching any `allowed_extension` within the given directory.
///
/// # Errors
///
/// Returns an error if Rust's underlying [`read_dir`](std::fs::read_dir) returns an error.
fn collect_files_with_extension(
    base_path: &Path,
    allowed_extension: &[&str],
) -> Result<Vec<PathBuf>, ConfigFileError> {
    Ok(base_path
        .read_dir()
        .map_err(|e| IoError(e, base_path.to_path_buf()))?
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

fn parse_url(is_enabled: bool, path: &Path) -> Result<Option<Url>, ConfigFileError> {
    if !is_enabled {
        return Ok(None);
    }
    path.to_str()
        .filter(|v| v.starts_with("http://") || v.starts_with("https://") || v.starts_with("s3://"))
        .map(|v| Url::parse(v).map_err(|e| InvalidSourceUrl(e, v.to_string())))
        .transpose()
}

pub type UnrecognizedValues = HashMap<String, serde_yaml::Value>;

pub fn copy_unrecognized_config(
    result: &mut UnrecognizedValues,
    prefix: &str,
    unrecognized: &UnrecognizedValues,
) {
    result.extend(
        unrecognized
            .iter()
            .map(|(k, v)| (format!("{prefix}{k}"), v.clone())),
    );
}
