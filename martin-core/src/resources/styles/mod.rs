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
#[cfg(all(feature = "rendering", target_os = "linux"))]
use maplibre_native::Image;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod error;
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use error::StyleError;

/// Style metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "unstable-schemas",
    derive(schemars::JsonSchema, utoipa::ToSchema)
)]
pub struct CatalogStyleEntry {
    /// Path to the style JSON file.
    // utoipa 5.4 has no native `PathBuf` support — present it as a `String`
    // (the on-the-wire form) for both schema generators.
    #[cfg_attr(feature = "unstable-schemas", schemars(with = "String"))]
    #[cfg_attr(feature = "unstable-schemas", schema(value_type = String))]
    pub path: PathBuf,
}

/// Catalog mapping style names to metadata (e.g., "basic" -> `CatalogStyleEntry`).
pub type StyleCatalog = HashMap<String, CatalogStyleEntry>;

/// Thread-safe style source manager.
#[derive(Debug, Clone, Default)]
pub struct StyleSources {
    sources: DashMap<String, StyleSource>,
    // if rendering is allowed
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    rendering_enabled: bool,
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
                },
            );
        }
        entries
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
    /// Assumptions:
    /// - martin is not an interactive renderer (think 60fps, embedded)
    /// - We are not rendering the same tile all the time (instead, it is cached)
    ///
    /// For now, we only use a static renderer which is optimized for our kind of usage
    /// In the future, we may consider adding support for smarter rendering including a pool of renderers.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    pub async fn render(&self, path: PathBuf, z: u8, x: u32, y: u32) -> Result<Image, StyleError> {
        if !self.rendering_enabled {
            return Err(StyleError::RenderingIsDisabled);
        }

        let image = maplibre_native::SingleThreadedRenderPool::global_pool()
            .render_tile(path, z, x, y)
            .await?;
        Ok(image)
    }

    /// Enable or disable rendering.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    pub fn set_rendering_enabled(&mut self, arg: bool) {
        self.rendering_enabled = arg;
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[cfg(all(feature = "rendering", target_os = "linux"))]
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

    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[rstest]
    #[case::maplibre_demo_png("maplibre_demo.json", (0, 0, 0), image::ImageFormat::Png, "png")]
    #[case::maplibre_demo_zoom1_png("maplibre_demo.json", (1, 0, 0), image::ImageFormat::Png, "png")]
    #[case::maptiler_basic_png("src2/maptiler_basic.json", (0, 0, 0), image::ImageFormat::Png, "png")]
    #[case::maplibre_demo_jpeg("maplibre_demo.json", (0, 0, 0), image::ImageFormat::Jpeg, "jpeg")]
    #[case::maplibre_demo_zoom1_jpeg("maplibre_demo.json", (1, 0, 0), image::ImageFormat::Jpeg, "jpeg")]
    #[case::maptiler_basic_jpeg("src2/maptiler_basic.json", (0, 0, 0), image::ImageFormat::Jpeg, "jpeg")]
    #[tokio::test]
    async fn test_render_tile_with_fixtures(
        #[case] style_file: &str,
        #[case] (z, x, y): (u8, u32, u32),
        #[case] format: image::ImageFormat,
        #[case] ext: &str,
    ) {
        let style_dir = Path::new("../tests/fixtures/styles/");
        let style_path = style_dir.join(style_file);
        let mut styles = StyleSources::default();
        styles.set_rendering_enabled(true);

        let rendered = styles.render(style_path, z, x, y).await.unwrap();
        let rendered_img = rendered.as_image();
        let (width, height) = (rendered_img.width(), rendered_img.height());

        // Verify rendered tile dimensions are 512x512
        assert_eq!((width, height), (512, 512), "Rendered tile must be 512x512");

        // Verify the image is not blank (has at least 2 distinct pixel values)
        let pixels: std::collections::HashSet<_> = rendered_img.pixels().copied().collect();
        assert!(
            pixels.len() > 1,
            "Rendered image is blank (all pixels identical)"
        );

        // Encode rendered image to the target format
        // JPEG doesn't support alpha, so convert RGBA->RGB when needed
        let encoded_img: image::DynamicImage = if format == image::ImageFormat::Jpeg {
            image::DynamicImage::ImageRgb8(
                image::DynamicImage::ImageRgba8(rendered_img.clone()).to_rgb8(),
            )
        } else {
            image::DynamicImage::ImageRgba8(rendered_img.clone())
        };
        let mut rendered_buf = std::io::Cursor::new(Vec::new());
        encoded_img.write_to(&mut rendered_buf, format).unwrap();
        let rendered_bytes = rendered_buf.into_inner();

        // Verify format magic bytes
        match format {
            image::ImageFormat::Png => {
                assert!(
                    rendered_bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]),
                    "Encoded bytes are not valid PNG (wrong magic)"
                );
            }
            image::ImageFormat::Jpeg => {
                assert!(
                    rendered_bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
                    "Encoded bytes are not valid JPEG (wrong magic)"
                );
            }
            _ => {}
        }

        // Load reference image from git-tracked fixtures
        let reference_name = format!(
            "{}_{}_{}_{}.{ext}",
            style_file.replace('/', "_").replace(".json", ""),
            z,
            x,
            y
        );
        let reference_path =
            Path::new("../tests/fixtures/rendering_references").join(&reference_name);

        // Reference image MUST exist - if missing, the test fails and stores it on disk
        let reference_bytes = std::fs::read(&reference_path).unwrap_or_else(|_| {
                std::fs::create_dir_all(reference_path.parent().unwrap()).unwrap();
                // Sanity check: refuse to bless tiny or blank images
                assert!(
                    rendered_bytes.len() > 1000,
                    "Refusing to bless suspiciously small image ({} bytes)",
                    rendered_bytes.len()
                );
                std::fs::write(&reference_path, &rendered_bytes).unwrap();
                panic!(
                    "Created new reference image at {reference_path:?}. Commit this file and re-run the test."
                );
        });

        // image-compare's hybrid algorithm operates on RgbaImage directly.
        let rendered_for_cmp = image::load_from_memory_with_format(&rendered_bytes, format)
            .unwrap()
            .to_rgba8();
        let reference_for_cmp = image::load_from_memory_with_format(&reference_bytes, format)
            .unwrap()
            .to_rgba8();

        let similarity = image_compare::rgba_hybrid_compare(&reference_for_cmp, &rendered_for_cmp)
            .unwrap_or_else(|e| panic!("image_compare failed: {e}"));

        // Score is 1.0 for identical images; JPEG is lossy, so allow a lower minimum.
        let min_similarity = if format == image::ImageFormat::Jpeg {
            0.95
        } else {
            0.99
        };
        let score = similarity.score;
        assert!(
            score >= min_similarity,
            "Rendered image {reference_name} differs from reference: similarity score {score:.4} < {min_similarity}. \
             If this is expected, delete the existing reference file and regenerate it using the current rendering output.",
        );
    }
}
