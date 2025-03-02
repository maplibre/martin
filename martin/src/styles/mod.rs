use dashmap::{DashMap, Entry};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use crate::config::UnrecognizedValues;
use crate::file_config::{ConfigExtras, FileConfigEnum, FileResult};

pub type StyleResult<T> = Result<T, StyleError>;

#[derive(thiserror::Error, Debug)]
pub enum StyleError {
    #[error("Style {0} not found")]
    StyleNotFound(String),

    #[error("IO error {0}: {1}")]
    IoError(std::io::Error, PathBuf),

    #[error("Style path is not a file: {0}")]
    InvalidFilePath(PathBuf),

    #[error("Style {0} uses bad file {1}")]
    InvalidStyleFilePath(String, PathBuf),

    #[error("No sprite files found in {0}")]
    NoStyleFilesFound(PathBuf),

    #[error("Style {1} could not be loaded because {0}")]
    UnableToReadStyle(serde_json::Error, PathBuf),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogStyleEntry {
    pub styles: Vec<String>,
}

pub type StyleCatalog = DashMap<String, CatalogStyleEntry>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for StyleConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

#[derive(Debug, Clone, Default)]
#[cfg_attr(test, serde_with::skip_serializing_none, derive(serde::Serialize))]
pub struct StyleSources(DashMap<String, StyleSource>);

#[derive(Clone, Debug)]
#[cfg_attr(test, serde_with::skip_serializing_none, derive(serde::Serialize))]
pub struct StyleSource {
    path: PathBuf,
}

impl StyleSources {
    pub fn resolve(config: &mut FileConfigEnum<StyleConfig>) -> FileResult<Self> {
        let Some(cfg) = config.extract_file_config(None)? else {
            return Ok(Self::default());
        };

        let mut results = Self::default();
        let mut paths = Vec::new();
        let mut configs = BTreeMap::new();

        if let Some(sources) = cfg.sources {
            for (id, source) in sources {
                configs.insert(id.clone(), source.clone());
                results.add_sources(&id, &source.abs_path()?);
            }
        };

        for path in cfg.paths {
            let Some(name) = path.file_name() else {
                warn!("Ignoring style source with no name from {path:?}");
                continue;
            };
            paths.push(path.clone());
            results.add_sources(name.to_string_lossy().as_ref(), &path);
        }

        *config = FileConfigEnum::new_extended(paths, configs, cfg.custom);

        Ok(results)
    }

    /// gets the style path from the internal catalog
    pub fn style_json_path(&self, style_id: &str) -> Option<PathBuf> {
        let item = self.0.get(style_id)?;
        Some(item.path.clone())
    }

    pub fn get_catalog(&self) -> StyleResult<StyleCatalog> {
        let entries = StyleCatalog::new();
        for source in &self.0 {
            let paths = get_input_paths(&source.path)?;
            let mut styles = Vec::with_capacity(paths.len());
            for path in paths {
                let name = parse_name(&path, &source.path).map_err(StyleError::InvalidFilePath)?;
                styles.push(name);
            }
            styles.sort();
            entries.insert(source.key().clone(), CatalogStyleEntry { styles });
        }
        Ok(entries)
    }

    fn add_single_source(&mut self, id: String, path: PathBuf) {
        match self.0.entry(id) {
            Entry::Occupied(v) => {
                warn!("Ignoring duplicate style source {id} from {new_path} because it was already configured for {old_path}",
                id=v.key(), old_path=v.get().path.display(), new_path=path.display());
            }
            Entry::Vacant(v) => {
                info!(
                    "Configured style source {id} to {new_path}",
                    id = v.key(),
                    new_path = path.display()
                );
                v.insert(StyleSource { path });
            }
        }
    }

    fn add_sources(&mut self, id: &str, base_path: &PathBuf) {
        match get_input_paths(base_path) {
            Ok(contained_paths) => {
                for child_path in contained_paths {
                    let name = parse_name(&child_path, base_path)
                        .expect("both child and base exist and child is contained in the base");
                    self.add_single_source(name, child_path);
                }
            }
            Err(e) => warn!("Ignoring style source {id} from {base_path:?} because of {e}"),
        }
    }
}

/// Returns `true` if `entry`'s file name starts with `.`, `false` otherwise.
fn is_hidden(entry: &Path) -> bool {
    let Some(name) = entry.file_name() else {
        return false;
    };
    name.to_str().is_some_and(|s| s.starts_with('.'))
}

/// Returns a vector of file paths in a given directory (or file)
///
/// It ignores hidden files (files whose names begin with `.`) but it does follow symlinks.
/// Will also return file paths in sub-directories recursively.
///
/// # Errors
///
/// This function will return an error if Rust's underlying [`read_dir`](std::fs::read_dir) returns an error.
pub fn get_input_paths(source_path: &Path) -> StyleResult<Vec<PathBuf>> {
    let paths = source_path
        .read_dir()
        .map_err(|e| StyleError::IoError(e, source_path.to_path_buf()))?;
    let mut input_paths = Vec::new();
    for path in paths {
        let path = path
            .map_err(|e| StyleError::IoError(e, source_path.to_path_buf()))?
            .path();
        if path.is_file() && !is_hidden(&path) {
            input_paths.push(path);
        } else if path.is_dir() {
            input_paths.extend(get_input_paths(&path)?);
        }
    }
    Ok(input_paths)
}

/// Returns the name (unique id) taken from a file.
///
/// The unique sprite name is the relative path from `path` to `base_path`
/// without the file extension.
///
/// # Errors
///
/// This function will return an error if:
///
/// - `path` does not exist
/// - `abs_path` does not exist
/// - `abs_path` is not an ancestor of `path`
pub fn parse_name(path: &PathBuf, base_path: &PathBuf) -> Result<String, PathBuf> {
    let abs_path = path.canonicalize().map_err(|_| path)?;
    let abs_base_path = base_path.canonicalize().map_err(|_| base_path)?;
    let Ok(rel_path) = abs_path.strip_prefix(abs_base_path) else {
        return Err(path.clone());
    };

    let Some(file_stem) = path.file_stem() else {
        return Err(path.clone());
    };
    if let Some(parent) = rel_path.parent() {
        Ok(parent.join(file_stem).to_string_lossy().to_string())
    } else {
        Ok(file_stem.to_string_lossy().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[actix_rt::test]
    async fn test_styles_resolve() {
        let mut cfg = FileConfigEnum::new(vec![
            PathBuf::from("../tests/fixtures/styles/americana.json"),
            PathBuf::from("../tests/fixtures/styles/src2"),
        ]);

        let styles = StyleSources::resolve(&mut cfg).unwrap();
        assert_eq!(styles.0.len(), 2);
        insta::assert_yaml_snapshot!(styles, @r#"
        maptiler_basic:
          path: "../tests/fixtures/styles/src2/maptiler_basic.json"
        navigatum-basemap:
          path: "../tests/fixtures/styles/src2/navigatum-basemap.json"
        "#);

        let catalog = styles.get_catalog().unwrap();
        insta::assert_yaml_snapshot!(catalog, @r#""#);
    }

    #[test]
    fn test_is_hidden() {
        assert!(is_hidden(&PathBuf::from(".hidden_file")));
        assert!(!is_hidden(&PathBuf::from("visible_file")));
    }

    #[test]
    fn test_get_input_paths() {
        use std::fs::File;
        let dir = tempfile::tempdir().unwrap();
        let file1 = dir.path().join("file1.txt");
        let hidden_file = dir.path().join(".hidden.txt");
        File::create(&file1).unwrap();
        File::create(&hidden_file).unwrap();

        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let file2 = subdir.join("file2.txt");
        File::create(&file2).unwrap();

        let mut result = get_input_paths(&dir.path().to_path_buf()).unwrap();
        result.sort();
        assert_eq!(result, vec![file1, file2]);
    }

    #[test]
    fn test_get_input_paths_error() {
        let result = get_input_paths(&PathBuf::from("/non_existent"));
        assert!(result.is_err());
    }
}
