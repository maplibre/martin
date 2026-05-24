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
#[cfg(all(feature = "rendering", target_os = "linux"))]
use std::num::NonZeroUsize;
use std::path::PathBuf;

use chrono::{DateTime, Utc};
use dashmap::{DashMap, Entry};
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use maplibre_native::Image as StaticImage;
#[cfg(all(feature = "rendering", target_os = "linux"))]
use maplibre_native::Image;
#[cfg(all(feature = "rendering", target_os = "linux"))]
use martin_tile_utils::tile_center_lng_lat;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tracing::{info, warn};

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod error;
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use error::StyleError;

/// Worker pool for map image rendering.
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub mod render_pool;
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use render_pool::{RenderParams, RendererPool};

/// What kind of layers a `MapLibre` style draws.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[cfg_attr(
    feature = "unstable-schemas",
    derive(schemars::JsonSchema, utoipa::ToSchema)
)]
pub enum StyleKind {
    /// Style only references vector tile sources.
    Vector,
    /// Style only references raster tile sources.
    Raster,
    /// Style references both vector and raster tile sources.
    Hybrid,
}

/// Style metadata.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "unstable-schemas",
    derive(schemars::JsonSchema, utoipa::ToSchema)
)]
pub struct CatalogStyleEntry {
    /// Path to the style JSON file.
    // utoipa 5.4 has no native `PathBuf` support - present it as a `String`
    // (the on-the-wire form) for both schema generators.
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "String"))]
    #[cfg_attr(feature = "unstable-schemas", schema(value_type = String))]
    pub path: PathBuf,
    /// What kind of layers the style draws.
    #[serde(rename = "type")]
    pub kind: Option<StyleKind>,
    /// Hash identifying the current style revision.
    pub version_hash: Option<String>,
    /// Number of layers declared in the style JSON.
    pub layer_count: Option<u32>,
    /// Distinct colors referenced by the style, for preview swatches.
    pub colors: Option<Vec<String>>,
    /// Timestamp of the style file's last modification.
    pub last_modified_at: Option<DateTime<Utc>>,
}

/// Catalog mapping style names to metadata (e.g., "basic" -> `CatalogStyleEntry`).
pub type StyleCatalog = HashMap<String, CatalogStyleEntry>;

/// Thread-safe style source manager.
#[derive(Debug, Clone, Default)]
pub struct StyleSources {
    sources: DashMap<String, StyleSource>,
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    pool: Option<RendererPool>,
}

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
        let item = self.sources.get(style_id)?;
        Some(item.path.clone())
    }

    /// Returns a catalog of all style sources.
    #[must_use]
    pub fn get_catalog(&self) -> StyleCatalog {
        let mut entries = StyleCatalog::new();
        for source in &self.sources {
            entries.insert(
                source.key().clone(),
                CatalogStyleEntry {
                    path: source.path.clone(),
                    // FIXME: parse the style JSON and surface its `type` field.
                    kind: None,
                    // FIXME: hash the style JSON contents.
                    version_hash: None,
                    // FIXME: parse the style JSON and count its `layers` array.
                    layer_count: None,
                    // FIXME: walk the style JSON and collect referenced colors.
                    colors: None,
                    // FIXME: stat the style file and surface its mtime.
                    last_modified_at: None,
                },
            );
        }
        entries
    }

    /// Whether server-side style rendering is currently enabled.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[must_use]
    pub fn is_rendering_enabled(&self) -> bool {
        self.pool.is_some()
    }

    /// Adds a style JSON file with an ID to the catalog.
    pub fn add_style(&mut self, id: String, path: PathBuf) {
        debug_assert!(path.is_file());
        debug_assert!(!id.is_empty());
        match self.sources.entry(id) {
            Entry::Occupied(v) => {
                warn!(
                    source.id = %v.key(),
                    style.path.kept = %v.get().path.display(),
                    style.path.dropped = %path.display(),
                    "Ignoring duplicate style source: already configured for another path"
                );
            }
            Entry::Vacant(v) => {
                info!(
                    source.id = %v.key(),
                    style.path = %path.display(),
                    "Configured style source"
                );
                v.insert(StyleSource { path });
            }
        }
    }

    /// Returns the number of style sources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Returns true if the catalog is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }

    /// EXPERIMENTAL support for rendering styles.
    ///
    /// Renders a 512×512 tile by aiming the static renderer at the tile's
    /// geographic centre, computed by [`tile_center_lng_lat`].
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    pub async fn render(&self, path: PathBuf, z: u8, x: u32, y: u32) -> Result<Image, StyleError> {
        let (lng, lat) = tile_center_lng_lat(z, x, y);
        self.render_static(RenderParams::new(path, lat, lng, f64::from(z)))
            .await
    }

    /// Render a map image with free camera control.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    pub async fn render_static(&self, params: RenderParams) -> Result<Image, StyleError> {
        self.pool
            .as_ref()
            .ok_or(StyleError::RenderingIsDisabled)?
            .render(params)
            .await
    }

    /// Enable rendering by spawning a [`RendererPool`]. Replaces any existing pool.
    ///
    /// See [`RendererPool::new`] for the meaning of `workers`.
    ///
    /// # Errors
    ///
    /// Returns the OS error from [`std::thread::Builder::spawn`] if a worker
    /// thread cannot be started.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    pub fn enable_rendering(
        &mut self,
        workers: Option<NonZeroUsize>,
    ) -> Result<(), std::io::Error> {
        self.pool = Some(RendererPool::new(workers)?);
        Ok(())
    }

    /// Disable rendering. Subsequent render calls return [`StyleError::RenderingIsDisabled`].
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    pub fn disable_rendering(&mut self) {
        self.pool = None;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn test_style_external() {
        let style_dir = Path::new("../tests/fixtures/styles/");

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
        assert_eq!(styles.sources.len(), 3);

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
