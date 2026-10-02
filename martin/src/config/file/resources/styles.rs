use std::env;
use std::mem;
#[cfg(feature = "rendering")]
use std::num::{NonZeroU8, NonZeroUsize};
use std::path::{Path, PathBuf};

#[cfg(all(feature = "rendering", target_os = "linux"))]
use martin_core::styles::DEFAULT_RENDERERS_PER_WORKER;
use martin_core::styles::StyleSources;
use martin_core::walk_files;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::file::{
    CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks,
    FileConfig, UnrecognizedValues, subdirectories,
};
#[cfg(feature = "rendering")]
use crate::config::primitives::OptBoolObj;

#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct InnerStyleConfig {
    /// Allows static, server side, style rendering
    #[cfg(feature = "rendering")]
    #[serde(default, skip_serializing_if = "OptBoolObj::is_none")]
    pub rendering: OptBoolObj<RendererConfig>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}

#[cfg(feature = "rendering")]
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct RendererConfig {
    // Same effect as rendering: true|false shorthands
    enabled: bool,

    /// Number of render worker threads. Unset picks a platform default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workers: Option<NonZeroUsize>,

    /// Renderers each tile worker keeps loaded, one per style and pixel ratio.
    /// Beyond this, the least recently used is dropped. \[default: 8\]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &16))]
    pub renderers_per_worker: Option<NonZeroUsize>,

    /// Highest `@{n}x` pixel ratio the tile endpoint serves. \[default: 4\]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "unstable-schemas", schemars(example = &4))]
    pub max_pixel_ratio: Option<NonZeroU8>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}
pub type StyleConfig = FileConfig<InnerStyleConfig>;

impl StyleConfig {
    pub fn resolve(&mut self) -> ConfigFileResult<StyleSources> {
        #[cfg_attr(
            not(all(feature = "rendering", target_os = "linux")),
            expect(unused_mut)
        )]
        let mut results = StyleSources::default();
        #[cfg(all(feature = "rendering", target_os = "linux"))]
        match self.custom.rendering {
            OptBoolObj::NoValue | OptBoolObj::Bool(false) => results.disable_rendering(),
            OptBoolObj::Object(ref o) if !o.enabled => results.disable_rendering(),
            OptBoolObj::Bool(true) => {
                results
                    .enable_rendering(None, DEFAULT_RENDERERS_PER_WORKER)
                    .map_err(ConfigFileError::RendererPoolSpawnFailed)?;
            }
            OptBoolObj::Object(ref o) => {
                results
                    .enable_rendering(
                        o.workers,
                        o.renderers_per_worker
                            .unwrap_or(DEFAULT_RENDERERS_PER_WORKER),
                    )
                    .map_err(ConfigFileError::RendererPoolSpawnFailed)?;
                results.set_max_pixel_ratio(o.max_pixel_ratio);
            }
        }
        #[cfg(all(feature = "rendering", not(target_os = "linux")))]
        match self.custom.rendering {
            OptBoolObj::NoValue | OptBoolObj::Bool(false) => {}
            OptBoolObj::Object(ref o) if !o.enabled => {}
            OptBoolObj::Bool(true) | OptBoolObj::Object(_) => {
                warn!("rendering is configured, but only available in Linux builds. Ignoring it.");
            }
        }

        self.sources.retain(|id, source| {
            if source.get_path().is_file() {
                results.add_style(id.clone(), source.get_path().clone());
                true
            } else {
                warn!(
                    "style {id} (pointing to {source:?}) is not a file. To prevent footguns, we ignore directories for 'sources'. To use directories, specify them as 'paths' or specify each file in 'sources' instead."
                );
                false
            }
        });

        let mut paths_with_names = Vec::new();
        for base_path in mem::take(&mut self.paths) {
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
                    .to_owned();
                results.add_style(style_id, path);
                paths_with_names.push(base_path.clone());
            }
        }
        paths_with_names.sort_unstable();
        paths_with_names.dedup();
        self.paths = paths_with_names;

        for collection in &self.collections {
            for (project, dir) in subdirectories(collection)
                .map_err(|e| ConfigFileError::IoError(e, collection.clone()))?
            {
                for path in list_contained_files(&dir, "json")? {
                    let Some(stem) = path.file_stem() else {
                        continue;
                    };
                    let style_id = format!("{project}.{}", stem.to_string_lossy().trim());
                    results.add_style(style_id, path);
                }
            }
        }

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
    use std::collections::BTreeMap;

    use indoc::indoc;
    use martin_core::styles::StyleCatalog;

    use super::*;
    use crate::config::file::FileConfigSrc;

    /// The catalog keeps the paths as joined, so on Windows they hold backslashes.
    fn catalog_with_forward_slashes(styles: &StyleSources) -> StyleCatalog {
        let mut catalog = styles.get_catalog();
        for entry in catalog.values_mut() {
            entry.path = entry
                .path
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/")
                .into();
        }
        catalog
    }

    #[test]
    fn styles_parse_paths_only_without_rendering_field() {
        let yaml = indoc! {"
            paths:
              - /data
        "};
        let cfg: StyleConfig =
            serde_saphyr::from_str(yaml).expect("styles with only paths must parse");
        assert_eq!(cfg.paths, vec![PathBuf::from("/data")]);
    }

    #[cfg(feature = "rendering")]
    #[test]
    fn renderer_config_parses_workers() {
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

    #[cfg(feature = "rendering")]
    #[test]
    fn renderer_config_rejects_zero_workers() {
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

    #[cfg(feature = "rendering")]
    #[test]
    fn renderer_config_parses_max_pixel_ratio() {
        let yaml = indoc! {"
            rendering:
              enabled: true
              max_pixel_ratio: 2
        "};
        let cfg: InnerStyleConfig =
            serde_saphyr::from_str(yaml).expect("rendering with max_pixel_ratio must parse");
        let OptBoolObj::Object(renderer) = cfg.rendering else {
            panic!("expected Object variant, got {:?}", cfg.rendering);
        };
        assert_eq!(renderer.max_pixel_ratio, NonZeroU8::new(2));
    }

    #[cfg(feature = "rendering")]
    #[rstest::rstest]
    #[case::zero("0")]
    #[case::above_u8("256")]
    #[case::negative("-1")]
    fn renderer_config_rejects_invalid_max_pixel_ratio(#[case] value: &str) {
        let yaml = format!("rendering:\n  enabled: true\n  max_pixel_ratio: {value}\n");
        serde_saphyr::from_str::<InnerStyleConfig>(&yaml)
            .expect_err("max_pixel_ratio must be an integer in 1..=255");
    }

    #[cfg(feature = "rendering")]
    #[test]
    fn renderer_config_parses_renderers_per_worker() {
        let yaml = indoc! {"
            rendering:
              enabled: true
              renderers_per_worker: 32
        "};
        let cfg: InnerStyleConfig =
            serde_saphyr::from_str(yaml).expect("rendering with renderers_per_worker must parse");
        let OptBoolObj::Object(renderer) = cfg.rendering else {
            panic!("expected Object variant, got {:?}", cfg.rendering);
        };
        assert_eq!(renderer.renderers_per_worker, NonZeroUsize::new(32));
    }

    #[cfg(feature = "rendering")]
    #[test]
    fn renderer_config_rejects_zero_renderers_per_worker() {
        let yaml = indoc! {"
            rendering:
              enabled: true
              renderers_per_worker: 0
        "};
        serde_saphyr::from_str::<InnerStyleConfig>(yaml)
            .expect_err("renderers_per_worker: 0 must be rejected by NonZeroUsize");
    }

    #[test]
    fn styles_resolve_paths() {
        let style_dir = Path::new("../tests/fixtures/styles/");
        let mut cfg = StyleConfig::new(vec![
            style_dir.join("maplibre_demo.json"),
            style_dir.join("src2"),
        ]);

        let styles = cfg.resolve().unwrap();
        assert_eq!(styles.len(), 3);
        insta::with_settings!({sort_maps => true}, {
        insta::assert_yaml_snapshot!(catalog_with_forward_slashes(&styles), @r#"
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
    fn styles_resolve_sources() {
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
            .map(|(k, v)| (k.to_owned(), FileConfigSrc::Path(v)))
            .collect();
        let mut cfg = StyleConfig {
            sources: configs,
            ..StyleConfig::default()
        };

        let styles = cfg.resolve().unwrap();
        assert_eq!(styles.len(), 2);
        insta::with_settings!({sort_maps => true}, {
        insta::assert_yaml_snapshot!(catalog_with_forward_slashes(&styles), @r#"
        maplibre_demo:
          path: "../tests/fixtures/styles/maplibre_demo.json"
        osm-liberty-lite:
          path: "../tests/fixtures/styles/src2/osm-liberty-lite.json"
        "#);
        });
    }

    #[test]
    fn style_external() {
        let style_dir = Path::new("../tests/fixtures/styles/");
        let mut cfg = StyleConfig::new(vec![
            style_dir.join("maplibre_demo.json"),
            style_dir.join("src2"),
        ]);

        let styles = cfg.resolve().unwrap();
        assert_eq!(styles.len(), 3);

        let catalog = catalog_with_forward_slashes(&styles);

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
    fn lists_contained_files() {
        use std::fs::File;
        let dir = tempfile::tempdir().unwrap();

        let file1 = dir.path().join("file1.txt");
        File::create(&file1).unwrap();
        let hidden_file2 = dir.path().join(".hidden.txt");
        File::create(&hidden_file2).unwrap();

        let subdir = dir.path().join("subdir");
        std::fs::create_dir_all(&subdir).unwrap();
        let subdir_file2 = subdir.join("file2.txt");
        File::create(&subdir_file2).unwrap();

        let hidden_subdir2 = dir.path().join(".subdir2");
        std::fs::create_dir_all(&hidden_subdir2).unwrap();
        let transitively_hidden_file3 = hidden_subdir2.join("file3.txt");
        File::create(&transitively_hidden_file3).unwrap();

        let mut result = list_contained_files(dir.path(), "txt").unwrap();
        result.sort();
        assert_eq!(result, vec![file1, subdir_file2]);
    }

    #[test]
    fn list_contained_files_error() {
        let result = list_contained_files(Path::new("/non_existent"), "txt");
        result.unwrap_err();
    }
}
