use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use log::warn;
#[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
use martin_core::config::OptBoolObj;
use martin_core::styles::StyleSources;
use serde::{Deserialize, Serialize};

use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, FileConfigEnum,
    UnrecognizedKeys, UnrecognizedValues,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InnerStyleConfig {
    /// Allows static, server side, style rendering
    ///
    /// Note on EXPERIMENTAL status:
    /// We are not currently happy with the performance of this endpoint and intend to improve this in the future
    /// Marking this experimental means that we are not stuck with single threaded performance as a default until v2.0
    #[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
    pub experimental_rendering: OptBoolObj<RendererConfig>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for InnerStyleConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        #[allow(unused_mut)]
        let mut keys = self
            .unrecognized
            .keys()
            .cloned()
            .collect::<UnrecognizedKeys>();
        #[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
        match &self.experimental_rendering {
            OptBoolObj::NoValue | OptBoolObj::Bool(_) => {}
            OptBoolObj::Object(o) => keys.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("experimental_rendering.{k}")),
            ),
        }
        keys
    }
}

#[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RendererConfig {
    // Same effect as experimental_rendering: true|false shorthands
    enabled: bool,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}
#[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
impl ConfigurationLivecycleHooks for RendererConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

pub type StyleConfig = FileConfigEnum<InnerStyleConfig>;

impl StyleConfig {
    pub fn resolve(&mut self) -> ConfigFileResult<StyleSources> {
        let Some(cfg) = self.extract_file_config() else {
            return Ok(StyleSources::default());
        };

        let mut results = StyleSources::default();

        #[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
        match cfg.custom.experimental_rendering {
            OptBoolObj::NoValue | OptBoolObj::Bool(false) => results.set_rendering_enabled(false),
            OptBoolObj::Object(ref o) if !o.enabled => results.set_rendering_enabled(false),
            _ => {
                warn!(
                    "experimental feature rendering is enabled. Expect breaking changes in upcoming releases."
                );
                results.set_rendering_enabled(true);
            }
        }

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
