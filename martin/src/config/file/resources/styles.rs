use std::collections::BTreeMap;
use std::env;
#[cfg(all(feature = "rendering", target_os = "linux"))]
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use martin_core::styles::StyleSources;
use martin_core::walk_files;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, FileConfigEnum,
    UnrecognizedKeys, UnrecognizedValues,
};
#[cfg(all(feature = "rendering", target_os = "linux"))]
use crate::config::primitives::OptBoolObj;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct InnerStyleConfig {
    /// Allows static, server side, style rendering
    ///
    /// Note on EXPERIMENTAL status:
    /// We are not currently happy with the performance of this endpoint and intend to improve this in the future
    /// Marking this experimental means that we are not stuck with single threaded performance as a default until v2.0
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub rendering: OptBoolObj<RendererConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for InnerStyleConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        #[cfg_attr(
            not(all(feature = "rendering", target_os = "linux")),
            expect(unused_mut, reason = "to warn for unrecognized keys")
        )]
        let mut keys = self
            .unrecognized
            .keys()
            .cloned()
            .collect::<UnrecognizedKeys>();
        #[cfg(all(feature = "rendering", target_os = "linux"))]
        if let OptBoolObj::Object(o) = &self.rendering {
            keys.extend(
                o.get_unrecognized_keys()
                    .iter()
                    .map(|k| format!("rendering.{k}")),
            );
        }
        keys
    }
}

#[cfg(all(feature = "rendering", target_os = "linux"))]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct RendererConfig {
    // Same effect as rendering: true|false shorthands
    enabled: bool,

    /// Number of render worker threads. Unset picks a platform default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workers: Option<NonZeroUsize>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}
#[cfg(all(feature = "rendering", target_os = "linux"))]
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

        #[cfg(all(feature = "rendering", target_os = "linux"))]
        match cfg.custom.rendering {
            OptBoolObj::NoValue | OptBoolObj::Bool(false) => results.disable_rendering(),
            OptBoolObj::Object(ref o) if !o.enabled => results.disable_rendering(),
            OptBoolObj::Bool(true) => {
                warn!(
                    "experimental feature rendering is enabled. Expect breaking changes in upcoming releases."
                );
                results
                    .enable_rendering(None)
                    .map_err(ConfigFileError::RendererPoolSpawnFailed)?;
            }
            OptBoolObj::Object(ref o) => {
                warn!(
                    "experimental feature rendering is enabled. Expect breaking changes in upcoming releases."
                );
                results
                    .enable_rendering(o.workers)
                    .map_err(ConfigFileError::RendererPoolSpawnFailed)?;
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

        *self = Self::new_extended(paths_with_names, configs, cfg.custom);

        Ok(results)
    }
}

/// Returns matching file paths under `source_path`, rewritten relative to the
/// current working directory so the catalog displays portable, configurable
/// paths.
///
/// Walking semantics (recursion, hidden-file/dir skip, symlink following)
/// come from [`walk_files`].
///
/// # Errors
///
/// Returns an error if directory walking fails.
fn list_contained_files(
    source_path: &Path,
    filter_extension: &str,
) -> Result<Vec<PathBuf>, ConfigFileError> {
    let files = walk_files(source_path, &[filter_extension])
        .map_err(|e| ConfigFileError::DirectoryWalking(e, source_path.to_path_buf()))?;
    let working_directory = env::current_dir().ok();
    Ok(files
        .into_iter()
        .map(|path| match &working_directory {
            Some(work_dir) => path
                .strip_prefix(work_dir)
                .map(Path::to_path_buf)
                .unwrap_or(path),
            None => path,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;
    use crate::config::file::FileConfigSrc;

    #[test]
    fn test_styles_parse_paths_only_without_rendering_field() {
        let yaml = indoc! {"
            paths:
              - /data
        "};
        let cfg: StyleConfig =
            serde_saphyr::from_str(yaml).expect("styles with only paths must parse");
        let StyleConfig::Config(cfg) = cfg else {
            panic!("expected Config variant, got {cfg:?}");
        };
        let paths: Vec<_> = cfg.paths.into_iter().collect();
        assert_eq!(paths, vec![PathBuf::from("/data")]);
    }

    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[test]
    fn test_renderer_config_parses_workers() {
        use std::num::NonZeroUsize;
        let yaml = indoc! {"
            rendering:
              enabled: true
              workers: 4
        "};
        let cfg: InnerStyleConfig =
            serde_saphyr::from_str(yaml).expect("rendering with workers must parse");
        let OptBoolObj::Object(renderer) = cfg.rendering else {
            panic!("expected Object variant, got {:?}", cfg.rendering);
        };
        assert!(renderer.enabled);
        assert_eq!(renderer.workers, NonZeroUsize::new(4));
    }

    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[test]
    fn test_renderer_config_rejects_zero_workers() {
        let yaml = indoc! {"
            rendering:
              enabled: true
              workers: 0
        "};
        let err = serde_saphyr::from_str::<InnerStyleConfig>(yaml)
            .expect_err("workers: 0 must be rejected by NonZeroUsize");
        // sanity check that the error mentions the offending field/value
        let msg = err.to_string();
        assert!(
            msg.contains("workers") || msg.contains("zero") || msg.contains("NonZero"),
            "unexpected error message: {msg}"
        );
    }

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
        result.unwrap_err();
    }
}
