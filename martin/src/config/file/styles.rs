use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use log::warn;
use martin_core::styles::StyleSources;
use serde::{Deserialize, Serialize};

use crate::MartinResult;
use crate::config::file::{ConfigExtras, ConfigFileError, FileConfigEnum, UnrecognizedValues};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InnerStyleConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for InnerStyleConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

pub type StyleConfig = FileConfigEnum<InnerStyleConfig>;

impl StyleConfig {
    pub fn resolve(&mut self) -> MartinResult<StyleSources> {
        let Some(cfg) = self.extract_file_config(None)? else {
            return Ok(StyleSources::default());
        };

        let mut results = StyleSources::default();
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
                warn!(
                    "No styles (.json files) found in path {:?}",
                    base_path.display()
                );
                continue;
            }
            for path in files {
                let Some(name) = path.file_name() else {
                    warn!(
                        "Ignoring style source with no name from {:?}",
                        path.display()
                    );
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

        *self = FileConfigEnum::new_extended(paths_with_names, configs, cfg.custom);

        Ok(results)
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
fn list_contained_files(
    source_path: &Path,
    filter_extension: &str,
) -> Result<Vec<PathBuf>, ConfigFileError> {
    let working_directory = std::env::current_dir().ok();
    let mut contained_files = Vec::new();
    let it = walkdir::WalkDir::new(source_path)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| e.depth() == 0 || !is_hidden(e));
    for entry in it {
        let entry =
            entry.map_err(|e| ConfigFileError::DirectoryWalking(e, source_path.to_path_buf()))?;
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
    use super::*;
    use crate::config::file::FileConfigSrc;

    #[test]
    fn test_styles_resolve_paths() {
        let style_dir = Path::new("../tests/fixtures/styles/");
        let mut cfg = StyleConfig::new(vec![
            style_dir.join("maplibre_demo.json"),
            style_dir.join("src2"),
        ]);

        let styles = cfg.resolve().unwrap();
        assert_eq!(styles.len(), 3);
        insta::with_settings!({sort_maps => true}, {
        insta::assert_yaml_snapshot!(styles.get_catalog(), @r#"
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
        let style_dir = Path::new("../tests/fixtures/styles/");
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
        let mut cfg = StyleConfig::new_extended(vec![], configs, InnerStyleConfig::default());

        let styles = cfg.resolve().unwrap();
        assert_eq!(styles.len(), 2);
        insta::with_settings!({sort_maps => true}, {
        insta::assert_yaml_snapshot!(styles.get_catalog(), @r#"
            maplibre_demo:
              path: "../tests/fixtures/styles/maplibre_demo.json"
            osm-liberty-lite:
              path: "../tests/fixtures/styles/src2/osm-liberty-lite.json"
            "#);
        });
    }

    #[test]
    fn test_style_external() {
        let style_dir = Path::new("../tests/fixtures/styles/");
        let mut cfg = StyleConfig::new(vec![
            style_dir.join("maplibre_demo.json"),
            style_dir.join("src2"),
        ]);

        let styles = cfg.resolve().unwrap();
        assert_eq!(styles.len(), 3);

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
        let result = list_contained_files(Path::new("/non_existent"), "txt");
        assert!(result.is_err());
    }
}
