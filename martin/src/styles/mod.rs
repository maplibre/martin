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
    #[error("IO error {0}: {1}")]
    IoError(std::io::Error, PathBuf),
    #[error("Walk directory error {0}: {1}")]
    DirectoryWalking(walkdir::Error, PathBuf),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogStyleEntry {
    pub path: PathBuf,
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
        let mut configs = BTreeMap::new();

        if let Some(sources) = cfg.sources {
            for (id, source) in sources {
                configs.insert(id.clone(), source.clone());
                results.add_sources(&id, &source.abs_path()?);
            }
        };

        let mut paths_with_names = Vec::new();
        for path in cfg.paths {
            let Some(name) = path.file_name() else {
                warn!("Ignoring style source with no name from {path:?}");
                continue;
            };
            paths_with_names.push(path.clone());
            results.add_sources(name.to_string_lossy().as_ref(), &path);
        }

        *config = FileConfigEnum::new_extended(paths_with_names, configs, cfg.custom);

        Ok(results)
    }

    /// retrieve a styles' `PathBuf` from the internal catalog
    pub fn style_json_path(&self, style_id: &str) -> Option<PathBuf> {
        let style_id = style_id.trim_end_matches(".json").trim();
        let item = self.0.get(style_id)?;
        Some(item.path.clone())
    }

    /// an external representation of the internal catalog
    #[must_use]
    pub fn get_catalog(&self) -> StyleCatalog {
        let entries = StyleCatalog::new();
        for source in &self.0 {
            entries.insert(
                source.key().clone(),
                CatalogStyleEntry {
                    path: source.path.clone(),
                },
            );
        }
        entries
    }

    /// Adds all the contained files in the given file/directory as style sources.
    fn add_sources(&mut self, id: &str, base_path: &PathBuf) {
        match list_contained_files(base_path) {
            Ok(contained_paths) => {
                for child_path in contained_paths {
                    let Some(name) = child_path.file_name() else {
                        // FIXME: we should never panic in production code
                        assert!(
                            !base_path.is_file(),
                            "base_path cannot be a file without name as otherwise the id would not exist"
                        );
                        warn!(
                            "Ignoring {child_path:?} of style source {id} from {base_path:?} because it has no name"
                        );
                        continue;
                    };
                    let name = name
                        .to_string_lossy()
                        .trim_end_matches(".json")
                        .trim()
                        .to_string();
                    self.add_single_source(name, child_path);
                }
            }
            Err(e) => warn!("Ignoring style source {id} from {base_path:?} because of {e}"),
        }
    }

    /// add a single file with an id to the internal catalog
    fn add_single_source(&mut self, id: String, path: PathBuf) {
        assert!(path.is_file());
        assert!(!id.is_empty());
        match self.0.entry(id) {
            Entry::Occupied(v) => {
                warn!(
                    "Ignoring duplicate style source {id} from {new_path} because it was already configured for {old_path}",
                    id = v.key(),
                    old_path = v.get().path.display(),
                    new_path = path.display()
                );
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
}

/// Returns a vector of file paths in a given directory (or file)
///
/// It ignores hidden files (files whose names begin with `.`) but it does follow symlinks.
/// Will also return file paths in sub-directories recursively.
///
/// # Errors
///
/// This function will return an error if Rust's underlying [`read_dir`](std::fs::read_dir) returns an error.
fn list_contained_files(source_path: &Path) -> StyleResult<Vec<PathBuf>> {
    let working_directory = std::env::current_dir().ok();
    let mut contained_files = Vec::new();
    let it = walkdir::WalkDir::new(source_path)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e));
    for entry in it {
        let entry =
            entry.map_err(|e| StyleError::DirectoryWalking(e, source_path.to_path_buf()))?;
        if entry.path().is_file() {
            // path should be relative to the working directory in the catalog
            let relative_path = match working_directory {
                Some(ref work_dir) => entry
                    .path()
                    .strip_prefix(work_dir.as_path())
                    .unwrap_or_else(|_| entry.path())
                    .to_owned(),
                None => entry.into_path(),
            };
            contained_files.push(relative_path);
        }
    }
    Ok(contained_files)
}

/// Returns `true` if `entry`'s file name starts with `.`, `false` otherwise.
fn is_hidden(entry: &walkdir::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|s| s.starts_with('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn test_add_single_source() {
        use std::fs::File;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("maplibre_demo.json");
        File::create(path.clone()).unwrap();

        let mut style_sources = StyleSources::default();
        style_sources.add_single_source("maplibre_demo".to_string(), path.clone());

        assert_eq!(style_sources.0.len(), 1);
        let maplibre_demo = style_sources.0.get("maplibre_demo").unwrap();
        assert_eq!(maplibre_demo.path, path);
    }

    #[actix_rt::test]
    async fn test_styles_resolve() {
        let style_dir = PathBuf::from("../tests/fixtures/styles/");
        let mut cfg = FileConfigEnum::new(vec![
            style_dir.join("maplibre_demo.json"),
            style_dir.join("src2"),
        ]);

        let styles = StyleSources::resolve(&mut cfg).unwrap();
        assert_eq!(styles.0.len(), 3);
        insta::with_settings!({sort_maps => true}, {
        insta::assert_yaml_snapshot!(styles, @r#"
        maplibre_demo:
          path: "../tests/fixtures/styles/maplibre_demo.json"
        maptiler_basic:
          path: "../tests/fixtures/styles/src2/maptiler_basic.json"
        osm-liberty-lite:
          path: "../tests/fixtures/styles/src2/osm-liberty-lite.json"
        "#);
        });
    }

    #[actix_rt::test]
    async fn test_style_external() {
        let style_dir = PathBuf::from("../tests/fixtures/styles/");
        let mut cfg = FileConfigEnum::new(vec![
            style_dir.join("maplibre_demo.json"),
            style_dir.join("src2"),
        ]);

        let styles = StyleSources::resolve(&mut cfg).unwrap();
        assert_eq!(styles.0.len(), 3);

        let catalog = styles.get_catalog();

        insta::with_settings!({sort_maps => true}, {
        insta::assert_json_snapshot!(catalog, @r#"
        {
          "maplibre_demo": {
            "path": "../tests/fixtures/styles/maplibre_demo.json"
          },
          "maptiler_basic": {
            "path": "../tests/fixtures/styles/src2/maptiler_basic.json"
          },
          "osm-liberty-lite": {
            "path": "../tests/fixtures/styles/src2/osm-liberty-lite.json"
          }
        }
        "#);
        });

        assert_eq!(styles.style_json_path("NON_EXISTENT"), None);
        assert_eq!(
            styles.style_json_path("maplibre_demo.json"),
            Some(style_dir.join("maplibre_demo.json"))
        );
        assert_eq!(styles.style_json_path("src2"), None);
        let src2_dir = style_dir.join("src2");
        assert_eq!(
            styles.style_json_path("maptiler_basic"),
            Some(src2_dir.join("maptiler_basic.json"))
        );
        assert_eq!(
            styles.style_json_path("maptiler_basic.json"),
            Some(src2_dir.join("maptiler_basic.json"))
        );
        assert_eq!(
            styles.style_json_path("osm-liberty-lite.json"),
            Some(src2_dir.join("osm-liberty-lite.json"))
        );
    }

    #[test]
    fn test_list_contained_files() {
        use std::fs::File;
        let dir = tempfile::tempdir().unwrap();

        let file1 = dir.path().join("file1.txt");
        File::create(&file1).unwrap();
        let hidden_file2 = dir.path().join(".hidden.txt");
        File::create(&hidden_file2).unwrap();

        let subdir = dir.path().join("subdir");
        std::fs::create_dir(&subdir).unwrap();
        let subdir_file2 = subdir.join("file2.txt");
        File::create(&subdir_file2).unwrap();

        let hidden_subdir2 = dir.path().join(".subdir2");
        std::fs::create_dir(&hidden_subdir2).unwrap();
        let transitively_hidden_file3 = hidden_subdir2.join("file3.txt");
        File::create(&transitively_hidden_file3).unwrap();

        let mut result = list_contained_files(dir.path()).unwrap();
        result.sort();
        assert_eq!(result, vec![file1, subdir_file2]);
    }

    #[test]
    fn test_list_contained_files_error() {
        let result = list_contained_files(&PathBuf::from("/non_existent"));
        assert!(result.is_err());
    }
}
