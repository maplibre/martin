//! Style processing and serving for map tile rendering.
//!
//! Manages `MapLibre` style JSON files for map rendering clients.
//!
//! # Usage
//!
//! ```rust,no_run
//! use martin_core::styles::StyleSources;
//! use std::path::PathBuf;
//!
//! let mut sources = StyleSources::default();
//! sources.add_style("basic".to_string(), PathBuf::from("/path/to/style.json"));
//! let path = sources.style_json_path("basic").unwrap();
//! ```

use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;

use dashmap::{DashMap, Entry};
use log::{info, warn};
use serde::{Deserialize, Serialize};

/// Style metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogStyleEntry {
    /// Path to the style JSON file.
    pub path: PathBuf,
}

/// Catalog mapping style names to metadata (e.g., "basic" -> `CatalogStyleEntry`).
pub type StyleCatalog = HashMap<String, CatalogStyleEntry>;

/// Thread-safe style source manager.
#[derive(Debug, Clone, Default)]
pub struct StyleSources(DashMap<String, StyleSource>);

/// Style source file.
#[derive(Clone, Debug)]
pub struct StyleSource {
    path: PathBuf,
}

impl StyleSources {
    /// Retrieve a style's path from the catalog.
    #[must_use]
    pub fn style_json_path(&self, style_id: &str) -> Option<PathBuf> {
        let style_id = style_id.trim_end_matches(".json").trim();
        let item = self.0.get(style_id)?;
        Some(item.path.clone())
    }

    /// Returns a catalog of all style sources.
    #[must_use]
    pub fn get_catalog(&self) -> StyleCatalog {
        let mut entries = StyleCatalog::new();
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

    /// Adds a style JSON file with an ID to the catalog.
    pub fn add_style(&mut self, id: String, path: PathBuf) {
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

    /// Returns the number of style sources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns true if the catalog is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_style_external() {
        let style_dir = PathBuf::from("../tests/fixtures/styles/");

        let mut styles = StyleSources::default();
        styles.add_style(
            "maplibre_demo".to_string(),
            style_dir.join("maplibre_demo.json"),
        );
        styles.add_style(
            "maptiler_basic".to_string(),
            style_dir.join("src2").join("maptiler_basic.json"),
        );
        styles.add_style(
            "osm-liberty-lite".to_string(),
            style_dir.join("src2").join("osm-liberty-lite.json"),
        );
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
}
