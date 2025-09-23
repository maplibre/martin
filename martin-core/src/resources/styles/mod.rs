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
#[cfg(feature = "render-styles")]
use maplibre_native::Image;
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

    /// EXPERIMENTAL support for rendering styles.
    ///
    /// Assumptions:
    /// - martin is not an interactive renderer (think 60fps, embedded)
    /// - We are not rendering the same tile all the time (instead, it is cached)
    ///
    /// For now, we only use a static renderer which is optimized for our kind of usage
    /// In the future, we may consider adding support for smarter rendering including a pool of renderers.
    #[cfg(feature = "render-styles")]
    pub async fn render(&self, path: &std::path::Path, zxy: martin_tile_utils::TileCoord) -> Image {
        use std::path::PathBuf;
        use std::sync::{LazyLock, mpsc};
        use std::thread;

        use tokio::sync::oneshot;

        struct RenderRequest {
            style_path: PathBuf,
            coord: martin_tile_utils::TileCoord,
            response: oneshot::Sender<Image>,
        }

        static RENDER_ACTOR: LazyLock<mpsc::Sender<RenderRequest>> = LazyLock::new(|| {
            let (tx, rx) = mpsc::channel::<RenderRequest>();

            thread::spawn(move || {
                let mut renderer =
                    maplibre_native::ImageRendererOptions::new().build_tile_renderer();
                let mut current_path = None;

                while let Ok(request) = rx.recv() {
                    // Switching styles, even if this were a no-op takes 250ms
                    if current_path.as_ref() != Some(&request.style_path) {
                        renderer.load_style_from_path(request.style_path.as_path());
                        current_path = Some(request.style_path.clone());
                    }
                    // TODO: if the style on disk is changed, we need to reload it via `load_style_from_path`

                    let image =
                        renderer.render_tile(request.coord.z, request.coord.x, request.coord.y);
                    let _ = request.response.send(image);
                }
            });

            tx
        });

        let (response_tx, response_rx) = oneshot::channel();
        let request = RenderRequest {
            style_path: path.to_path_buf(),
            coord: zxy,
            response: response_tx,
        };

        RENDER_ACTOR
            .send(request)
            .expect("Render actor should be alive");
        response_rx.await.expect("Render actor should respond")
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[cfg(feature = "render-styles")]
    use martin_tile_utils::TileCoord;
    #[cfg(feature = "render-styles")]
    use rstest::rstest;

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

    #[cfg(feature = "render-styles")]
    #[rstest]
    #[case::maplibre_demo("maplibre_demo.json", (0, 0, 0))]
    #[case::maplibre_demo_zoom1("maplibre_demo.json", (1, 0, 0))]
    #[case::maptiler_basic("src2/maptiler_basic.json", (0, 0, 0))]
    #[tokio::test]
    async fn test_render_tile_with_fixtures(
        #[case] style_file: &str,
        #[case] (z, x, y): (u8, u32, u32),
    ) {
        let style_dir = Path::new("../tests/fixtures/styles/");
        let style_path = style_dir.join(style_file);
        let styles = StyleSources::default();

        let coord = TileCoord { z, x, y };
        let image = styles.render(&style_path, coord).await;
        assert!(!image.as_slice().is_empty());

        // Create a snapshot name based on the style and coordinates
        let snapshot_name = format!(
            "{}_{}_{}_{}.png",
            style_file.replace('/', "_").replace(".json", ""),
            z,
            x,
            y
        );
        insta::assert_binary_snapshot!(&snapshot_name, image.as_slice().to_vec());
    }

    #[cfg(feature = "render-styles")]
    #[tokio::test(flavor = "multi_thread")]
    async fn test_render_concurrent_requests_no_side_effects() {
        let style_dir = Path::new("../tests/fixtures/styles/");
        let style_path = style_dir.join("maplibre_demo.json");
        let styles = StyleSources::default();

        let coords = [
            TileCoord { z: 0, x: 0, y: 0 },
            TileCoord { z: 1, x: 0, y: 0 },
            TileCoord { z: 1, x: 1, y: 0 },
            TileCoord { z: 1, x: 0, y: 1 },
        ];

        let futures = coords
            .iter()
            .map(|&coord| styles.render(&style_path, coord));

        let results = futures::future::join_all(futures).await;

        for (i, image) in results.iter().enumerate() {
            assert!(
                !image.as_slice().is_empty(),
                "Concurrent request {i} should produce non-empty image"
            );
        }
    }
}
