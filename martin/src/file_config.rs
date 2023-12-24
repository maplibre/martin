use std::collections::{BTreeMap, HashSet};
use std::fmt::Debug;
use std::future::Future;
use std::mem;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures::TryFutureExt;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::{copy_unrecognized_config, UnrecognizedValues};
use crate::file_config::FileError::{
    InvalidFilePath, InvalidSourceFilePath, InvalidSourceUrl, IoError,
};
use crate::source::{Source, TileInfoSources};
use crate::utils::{IdResolver, OptOneMany};
use crate::MartinResult;
use crate::OptOneMany::{Many, One};

pub type FileResult<T> = Result<T, FileError>;

#[derive(thiserror::Error, Debug)]
pub enum FileError {
    #[error("IO error {0}: {}", .1.display())]
    IoError(std::io::Error, PathBuf),

    #[error("Source path is not a file: {}", .0.display())]
    InvalidFilePath(PathBuf),

    #[error("Error {0} while parsing URL {1}")]
    InvalidSourceUrl(url::ParseError, String),

    #[error("Source {0} uses bad file {}", .1.display())]
    InvalidSourceFilePath(String, PathBuf),

    #[error(r"Unable to parse metadata in file {}: {0}", .1.display())]
    InvalidMetadata(String, PathBuf),

    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidUrlMetadata(String, Url),

    #[error(r#"Unable to aquire connection to file: {0}"#)]
    AquireConnError(String),

    #[error(r#"PMTiles error {0} processing {1}"#)]
    PmtError(pmtiles::PmtError, String),
}

#[async_trait]
pub trait FileConfigExtras: Clone + Debug + Default + PartialEq {
    // new_source: &mut impl FnMut(String, PathBuf) -> Fut1,
    // new_url_source: &mut impl FnMut(String, Url) -> Fut2,
    // ) -> FileResult<TileInfoSources>
    // where
    // Fut1: Future<Output = Result<Box<dyn Source>, FileError>>,
    // Fut2: Future<Output = Result<Box<dyn Source>, FileError>>,

    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<Box<dyn Source>>;
}
// impl<T: Clone + Debug + Default + PartialEq> FileConfigExtras for T {}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileConfigEnum<T: FileConfigExtras> {
    #[default]
    None,
    Path(PathBuf),
    Paths(Vec<PathBuf>),
    Config(FileConfig<T>),
}

impl<T: FileConfigExtras> FileConfigEnum<T> {
    #[must_use]
    pub fn new(paths: Vec<PathBuf>) -> FileConfigEnum<T> {
        Self::new_extended(paths, BTreeMap::new(), None, UnrecognizedValues::new())
    }

    #[must_use]
    pub fn new_extended(
        paths: Vec<PathBuf>,
        configs: BTreeMap<String, FileConfigSrc>,
        extras: Option<T>,
        unrecognized: UnrecognizedValues,
    ) -> Self {
        if configs.is_empty() && extras.is_none() && unrecognized.is_empty() {
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
                extras,
                unrecognized,
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
                paths: One(mem::take(path)),
                ..FileConfig::default()
            }),
            FileConfigEnum::Paths(paths) => Some(FileConfig {
                paths: Many(mem::take(paths)),
                ..Default::default()
            }),
            FileConfigEnum::Config(cfg) => Some(mem::take(cfg)),
        }
    }

    pub fn finalize(&self, prefix: &str) -> MartinResult<UnrecognizedValues> {
        let mut res = UnrecognizedValues::new();
        if let Self::Config(cfg) = self {
            copy_unrecognized_config(&mut res, prefix, &cfg.unrecognized);
        }
        Ok(res)
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
    #[serde(flatten)]
    pub extras: Option<T>,
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl<T> FileConfig<T> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_none() && self.sources.is_none()
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

    pub fn abs_path(&self) -> FileResult<PathBuf> {
        let path = self.get_path();
        path.canonicalize().map_err(|e| IoError(e, path.clone()))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FileConfigSource {
    pub path: PathBuf,
}

async fn dummy_resolver(_id: String, _url: Url) -> FileResult<Box<dyn Source>> {
    unreachable!()
}

pub async fn resolve_files<T: FileConfigExtras, Fut>(
    config: &mut FileConfigEnum<T>,
    idr: IdResolver,
    extension: &str,
    new_source: &mut impl FnMut(String, PathBuf) -> Fut,
) -> MartinResult<TileInfoSources>
where
    Fut: Future<Output = Result<Box<dyn Source>, FileError>>,
{
    let dummy = &mut dummy_resolver;
    resolve_int(config, idr, extension, false, new_source, dummy)
        .map_err(crate::MartinError::from)
        .await
}

pub async fn resolve_files_urls<T: FileConfigExtras>(
    config: &mut FileConfigEnum<T>,
    idr: IdResolver,
    extension: &str,
    // new_source: &mut impl FnMut(String, PathBuf) -> Fut1,
    // new_url_source: &mut impl FnMut(String, Url) -> Fut2,
) -> MartinResult<TileInfoSources> {
    resolve_int(config, idr, extension, true)
        .map_err(crate::MartinError::from)
        .await
}

async fn resolve_int<T: FileConfigExtras>(
    config: &mut FileConfigEnum<T>,
    idr: IdResolver,
    extension: &str,
    parse_urls: bool,
) -> FileResult<TileInfoSources> {
    let Some(cfg) = config.extract_file_config() else {
        return Ok(TileInfoSources::default());
    };

    let mut results = TileInfoSources::default();
    let mut configs = BTreeMap::new();
    let mut files = HashSet::new();
    let mut directories = Vec::new();

    if let Some(sources) = cfg.sources {
        for (id, source) in sources {
            if let Some(url) = parse_url(parse_urls, source.get_path())? {
                let dup = !files.insert(source.get_path().clone());
                let dup = if dup { "duplicate " } else { "" };
                let id = idr.resolve(&id, url.to_string());
                configs.insert(id.clone(), source);
                results.push(config.new_sources(id.clone(), url.clone()).await?);
                info!("Configured {dup}source {id} from {}", sanitize_url(&url));
            } else {
                let can = source.abs_path()?;
                if !can.is_file() {
                    // todo: maybe warn instead?
                    return Err(InvalidSourceFilePath(id.to_string(), can));
                }

                let dup = !files.insert(can.clone());
                let dup = if dup { "duplicate " } else { "" };
                let id = idr.resolve(&id, can.to_string_lossy().to_string());
                info!("Configured {dup}source {id} from {}", can.display());
                configs.insert(id.clone(), source.clone());
                results.push(new_source(id, source.into_path()).await?);
            }
        }
    }

    for path in cfg.paths {
        if let Some(url) = parse_url(parse_urls, &path)? {
            let id = url
                .path_segments()
                .and_then(Iterator::last)
                .and_then(|s| {
                    // Strip extension and trailing dot, or keep the original string
                    s.strip_suffix(extension)
                        .and_then(|s| s.strip_suffix('.'))
                        .or(Some(s))
                })
                .unwrap_or("pmt_web_source");

            let id = idr.resolve(id, url.to_string());
            configs.insert(id.clone(), FileConfigSrc::Path(path));
            results.push(new_url_source(id.clone(), url.clone()).await?);
            info!("Configured source {id} from URL {}", sanitize_url(&url));
        } else {
            let is_dir = path.is_dir();
            let dir_files = if is_dir {
                // directories will be kept in the config just in case there are new files
                directories.push(path.clone());
                dir_to_paths(&path, extension)?
            } else if path.is_file() {
                vec![path]
            } else {
                return Err(InvalidFilePath(path.canonicalize().unwrap_or(path)));
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
                results.push(new_source(id, path).await?);
            }
        }
    }

    *config = FileConfigEnum::new_extended(directories, configs, cfg.extras, cfg.unrecognized);

    Ok(results)
}

fn dir_to_paths(path: &Path, extension: &str) -> Result<Vec<PathBuf>, FileError> {
    Ok(path
        .read_dir()
        .map_err(|e| IoError(e, path.to_path_buf()))?
        .filter_map(Result::ok)
        .filter(|f| {
            f.path().extension().filter(|e| *e == extension).is_some() && f.path().is_file()
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

fn parse_url(is_enabled: bool, path: &Path) -> Result<Option<Url>, FileError> {
    if !is_enabled {
        return Ok(None);
    }
    path.to_str()
        .filter(|v| v.starts_with("http://") || v.starts_with("https://"))
        .map(|v| Url::parse(v).map_err(|e| InvalidSourceUrl(e, v.to_string())))
        .transpose()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use indoc::indoc;

    use crate::file_config::{FileConfigEnum, FileConfigSource, FileConfigSrc};
    use crate::mbtiles::MbtilesConfig;

    #[test]
    fn parse() {
        let cfg = serde_yaml::from_str::<FileConfigEnum<MbtilesConfig>>(indoc! {"
            paths:
              - /dir-path
              - /path/to/file2.ext
              - http://example.org/file.ext
            sources:
                pm-src1: /tmp/file.ext
                pm-src2:
                  path: /tmp/file.ext
                pm-src3: https://example.org/file3.ext
                pm-src4:
                  path: https://example.org/file4.ext
        "})
        .unwrap();
        let res = cfg.finalize("").unwrap();
        assert!(res.is_empty(), "unrecognized config: {res:?}");
        let FileConfigEnum::Config(cfg) = cfg else {
            panic!();
        };
        let paths = cfg.paths.clone().into_iter().collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/dir-path"),
                PathBuf::from("/path/to/file2.ext"),
                PathBuf::from("http://example.org/file.ext"),
            ]
        );
        assert_eq!(
            cfg.sources,
            Some(BTreeMap::from_iter(vec![
                (
                    "pm-src1".to_string(),
                    FileConfigSrc::Path(PathBuf::from("/tmp/file.ext"))
                ),
                (
                    "pm-src2".to_string(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: PathBuf::from("/tmp/file.ext"),
                    })
                ),
                (
                    "pm-src3".to_string(),
                    FileConfigSrc::Path(PathBuf::from("https://example.org/file3.ext"))
                ),
                (
                    "pm-src4".to_string(),
                    FileConfigSrc::Obj(FileConfigSource {
                        path: PathBuf::from("https://example.org/file4.ext"),
                    })
                ),
            ]))
        );
    }
}
