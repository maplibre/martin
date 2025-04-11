use dashmap::{DashMap, Entry};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::path::{Path, PathBuf};

use crate::config::UnrecognizedValues;
use crate::file_config::{ConfigExtras, FileConfigEnum, FileError, FileResult};

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
                if source.get_path().is_file() {
                    configs.insert(id.clone(), source.clone());
                    results.add_style(id, source.into_path());
                } else {
                    warn!(
                        "style {id} (pointing to {source:?}) is not a file. To prevent footguns, we ignore directories for 'sources'. To use directories, specify them as 'paths' or specify each file in 'sources' instead."
                    );
                }
            }
        }

        let mut paths_with_names = Vec::new();
        for base_path in cfg.paths {
            let files = list_contained_files(&base_path, "json")?;
            if files.is_empty() {
                warn!("No styles (.json files) found in path {base_path:?}");
                continue;
            }
            for path in files {
                let Some(name) = path.file_name() else {
                    warn!("Ignoring style source with no name from {path:?}");
                    continue;
                };
                let style_id = name
                    .to_string_lossy()
                    .trim_end_matches(".json")
                    .trim()
                    .to_string();
                results.add_style(style_id, path);
                paths_with_names.push(base_path.clone());
            }
        }
        paths_with_names.sort_unstable();
        paths_with_names.dedup();

        *config = FileConfigEnum::new_extended(paths_with_names, configs, cfg.custom);

        Ok(results)
    }

    /// retrieve a styles' `PathBuf` from the internal catalog
    #[must_use]
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

    /// add a single style file with an id to the internal catalog
    fn add_style(&mut self, id: String, path: PathBuf) {
        debug_assert!(path.is_file());
        debug_assert!(!id.is_empty());
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
fn list_contained_files(source_path: &Path, filter_extension: &str) -> FileResult<Vec<PathBuf>> {
    let working_directory = std::env::current_dir().ok();
    let mut contained_files = Vec::new();
    let it = walkdir::WalkDir::new(source_path)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e));
    for entry in it {
        let entry = entry.map_err(|e| FileError::DirectoryWalking(e, source_path.to_path_buf()))?;
        if entry.path().is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext == filter_extension)
        {
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
    use crate::file_config::FileConfigSrc;

    use super::*;
    #[test]
    fn test_add_single_source() {
        use std::fs::File;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("maplibre_demo.json");
        File::create(&path).unwrap();

        let mut style_sources = StyleSources::default();
        assert_eq!(style_sources.0.len(), 0);

        style_sources.add_style("maplibre_demo".to_string(), path.clone());
        assert_eq!(style_sources.0.len(), 1);
        let maplibre_demo = style_sources.0.get("maplibre_demo").unwrap();
        assert_eq!(maplibre_demo.path, path);
    }

    #[test]
    fn test_styles_resolve_paths() {
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

    #[test]
    fn test_styles_resolve_sources() {
        let style_dir = PathBuf::from("../tests/fixtures/styles/");
        let mut configs = BTreeMap::new();
        configs.insert("maplibre_demo", style_dir.join("maplibre_demo.json"));
        configs.insert("src_ignored_due_to_directory", style_dir.join("src2"));
        configs.insert(
            "osm-liberty-lite",
            style_dir.join("src2").join("osm-liberty-lite.json"),
        );
        let configs = configs
            .into_iter()
            .map(|(k, v)| (k.to_string(), FileConfigSrc::Path(v)))
            .collect();
        let mut cfg = FileConfigEnum::new_extended(vec![], configs, StyleConfig::default());

        let styles = StyleSources::resolve(&mut cfg).unwrap();
        assert_eq!(styles.0.len(), 2);
        insta::with_settings!({sort_maps => true}, {
        insta::assert_yaml_snapshot!(styles, @r#"
        maplibre_demo:
          path: "../tests/fixtures/styles/maplibre_demo.json"
        osm-liberty-lite:
          path: "../tests/fixtures/styles/src2/osm-liberty-lite.json"
        "#);
        });
    }

    #[test]
    fn test_style_external() {
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

        let mut result = list_contained_files(dir.path(), "txt").unwrap();
        result.sort();
        assert_eq!(result, vec![file1, subdir_file2]);
    }

    #[test]
    fn test_list_contained_files_error() {
        let result = list_contained_files(&PathBuf::from("/non_existent"), "txt");
        assert!(result.is_err());
    }
}
