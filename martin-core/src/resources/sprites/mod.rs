//! Sprite processing and serving for map tile rendering.
//!
//! Generates spritesheets from SVG files with support for high-DPI (@2x) and
//! SDF (Signed Distance Field) sprites for dynamic styling.
//!
//! # Usage
//!
//! ```rust,no_run
//! # async fn foo() {
//! use martin_core::sprites::SpriteSources;
//! use std::path::PathBuf;
//!
//! let mut sources = SpriteSources::default();
//! sources.add_source("icons".to_string(), PathBuf::from("/path/to/svg/directory"));
//! let spritesheet = sources.get_sprites("icons@2x", false).await.unwrap();
//! # }
//! ```

use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;

use dashmap::{DashMap, Entry};
use futures::future::try_join_all;
use log::{info, warn};
use serde::{Deserialize, Serialize};
pub use spreet::Spritesheet;
use spreet::resvg::usvg::{Options, Tree};
use spreet::{Sprite, SpritesheetBuilder, get_svg_input_paths, sprite_name};
use tokio::io::AsyncReadExt;

use self::SpriteError::{SpriteInstError, SpriteParsingError, SpriteProcessingError};

mod error;
pub use error::SpriteError;

/// Sprite source metadata.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogSpriteEntry {
    /// Available sprite image names.
    pub images: Vec<String>,
}

/// Catalog mapping sprite names to metadata (e.g., "icons" -> [`CatalogSpriteEntry`]).
pub type SpriteCatalog = HashMap<String, CatalogSpriteEntry>;

/// Thread-safe sprite source manager for serving sprites as `.png` or `.json`.
#[derive(Debug, Clone, Default)]
pub struct SpriteSources(DashMap<String, SpriteSource>);

impl SpriteSources {
    /// Returns a catalog of all sprite sources.
    pub fn get_catalog(&self) -> Result<SpriteCatalog, SpriteError> {
        // TODO: all sprite generation should be pre-cached
        let mut entries = SpriteCatalog::new();
        for source in &self.0 {
            let paths = get_svg_input_paths(&source.path, true)
                .map_err(|e| SpriteProcessingError(e, source.path.clone()))?;
            let mut images = Vec::with_capacity(paths.len());
            for path in paths {
                images.push(
                    sprite_name(&path, &source.path)
                        .map_err(|e| SpriteProcessingError(e, source.path.clone()))?,
                );
            }
            images.sort();
            entries.insert(source.key().clone(), CatalogSpriteEntry { images });
        }
        Ok(entries)
    }

    /// Adds a sprite source directory containing SVG files.
    /// Files are ignored - only directories accepted. Duplicates ignored with warning.
    /// Performs basic validation of SVG format before adding the source.
    pub fn add_source(&mut self, id: String, path: PathBuf) -> Result<(), SpriteError> {
        let disp_path = path.display();

        if path.is_file() {
            return Err(SpriteError::NotADirectory(path));
        }

        if !path.exists() {
            return Err(SpriteError::DirectoryNotFound(path));
        }

        match self.0.entry(id) {
            Entry::Occupied(v) => {
                warn!(
                    "Ignoring duplicate sprite source {} from {disp_path}: Already configured from {}",
                    v.key(),
                    v.get().path.display()
                );
            }
            Entry::Vacant(v) => {
                info!("Configured sprite source {} from {disp_path}", v.key());
                v.insert(SpriteSource { path });
            }
        }

        Ok(())
    }

    /// Validates a sprite source directory to ensure it contains valid SVG files.
    /// Checks include:
    /// - Directory existence and accessibility
    /// - Presence of SVG files
    /// - Basic SVG format validation
    pub async fn validate_source_directory(&self, path: &PathBuf) -> Result<(), SpriteError> {
        let disp_path = path.display();
        let on_err = |e| SpriteError::IoError(e, path.clone());

        // Check if path exists and get metadata
        let metadata = tokio::fs::metadata(path).await.map_err(on_err)?;

        if !metadata.is_dir() {
            return Err(SpriteError::NotADirectory(path.clone()));
        }

        let (total_files, svg_count, sprite_output_files) =
            Self::scan_directory_files(path).await?;

        let mut entries = tokio::fs::read_dir(path).await.map_err(on_err)?;
        while let Some(entry) = entries.next_entry().await.map_err(on_err)? {
            let entry_path = entry.path();
            if entry_path.is_file() {
                if let Some(extension) = entry_path.extension() {
                    if extension.to_string_lossy().to_lowercase() == "svg" {
                        self.validate_svg_file(&entry_path).await?;
                    }
                }
            }
        }

        if total_files == 0 {
            return Err(SpriteError::EmptyDirectory(path.clone()));
        }

        if svg_count == 0 && sprite_output_files.is_empty() {
            return Err(SpriteError::DirectoryValidationFailed(
                path.clone(),
                "Directory contains no SVG files".to_string(),
            ));
        }

        info!(
            "Validated sprite directory {disp_path}: found {svg_count} SVG files out of {total_files} total files"
        );
        Ok(())
    }

    /// Validates an individual SVG file for format.
    async fn validate_svg_file(&self, path: &PathBuf) -> Result<(), SpriteError> {
        let on_err = |e| SpriteError::IoError(e, path.clone());

        let content = tokio::fs::read_to_string(path).await.map_err(on_err)?;
        let content = content.trim();

        if content.is_empty() {
            return Err(SpriteError::EmptyFile(path.clone()));
        }

        if !content.starts_with("<?xml") && !content.starts_with("<svg") {
            return Err(SpriteError::InvalidSvgFormat(
                path.clone(),
                "Missing SVG declaration".to_string(),
            ));
        }

        // Check if it contains an SVG tag
        if !content.contains("<svg") {
            return Err(SpriteError::InvalidSvgFormat(
                path.clone(),
                "No SVG element found".to_string(),
            ));
        }

        Ok(())
    }

    /// Scans a directory and returns file counts.
    async fn scan_directory_files(
        path: &PathBuf,
    ) -> Result<(usize, usize, Vec<String>), SpriteError> {
        let on_err = |e| SpriteError::IoError(e, path.clone());
        let mut entries = tokio::fs::read_dir(path).await.map_err(on_err)?;
        let mut total_files = 0;
        let mut svg_count = 0;
        let mut sprite_output_files = Vec::new();

        while let Some(entry) = entries.next_entry().await.map_err(on_err)? {
            let entry_path = entry.path();
            if entry_path.is_file() {
                total_files += 1;
                if let Some(extension) = entry_path.extension() {
                    let ext = extension.to_string_lossy().to_lowercase();
                    if ext == "svg" {
                        svg_count += 1;
                    } else if ext == "png" || ext == "json" {
                        let filename = entry_path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if filename.contains("sprite") || filename.contains("@2x") {
                            sprite_output_files.push(
                                entry_path
                                    .file_name()
                                    .map(|s| s.to_string_lossy().to_string())
                                    .unwrap_or_default(),
                            );
                        }
                    }
                }
            }
        }

        Ok((total_files, svg_count, sprite_output_files))
    }
}

impl SpriteSources {
    /// Generates a spritesheet from comma-separated sprite source IDs.
    ///
    /// Append "@2x" for high-DPI sprites.
    /// Set `as_sdf` for SDF sprites.
    pub async fn get_sprites(&self, ids: &str, as_sdf: bool) -> Result<Spritesheet, SpriteError> {
        let (ids, dpi) = if let Some(ids) = ids.strip_suffix("@2x") {
            (ids, 2)
        } else {
            (ids, 1)
        };

        let sprite_ids = ids
            .split(',')
            .map(|id| self.get(id))
            .collect::<Result<Vec<_>, SpriteError>>()?;

        get_spritesheet(sprite_ids.iter(), dpi, as_sdf).await
    }

    fn get(&self, id: &str) -> Result<SpriteSource, SpriteError> {
        match self.0.get(id) {
            Some(v) => Ok(v.clone()),
            None => Err(SpriteError::SpriteNotFound(id.to_string())),
        }
    }
}

/// Sprite source directory.
#[derive(Clone, Debug)]
pub struct SpriteSource {
    path: PathBuf,
}

/// Parses SVG file into sprite.
async fn parse_sprite(
    name: String,
    path: PathBuf,
    pixel_ratio: u8,
    as_sdf: bool,
) -> Result<(String, Sprite), SpriteError> {
    let on_err = |e| SpriteError::IoError(e, path.clone());

    let mut file = tokio::fs::File::open(&path).await.map_err(on_err)?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await.map_err(on_err)?;

    let tree = Tree::from_data(&buffer, &Options::default())
        .map_err(|e| SpriteParsingError(e, path.clone()))?;

    let sprite = if as_sdf {
        Sprite::new_sdf(tree, pixel_ratio)
    } else {
        Sprite::new(tree, pixel_ratio)
    };
    let sprite = sprite.ok_or_else(|| SpriteInstError(path.clone()))?;

    Ok((name, sprite))
}

/// Generates spritesheet from sprite sources.
pub async fn get_spritesheet(
    sources: impl Iterator<Item = &SpriteSource>,
    pixel_ratio: u8,
    as_sdf: bool,
) -> Result<Spritesheet, SpriteError> {
    // Asynchronously load all SVG files from the given sources
    let mut futures = Vec::new();
    for source in sources {
        let paths = get_svg_input_paths(&source.path, true)
            .map_err(|e| SpriteProcessingError(e, source.path.clone()))?;
        for path in paths {
            let name = sprite_name(&path, &source.path)
                .map_err(|e| SpriteProcessingError(e, source.path.clone()))?;
            futures.push(parse_sprite(name, path, pixel_ratio, as_sdf));
        }
    }
    let sprites = try_join_all(futures).await?;
    let mut builder = SpritesheetBuilder::new();
    if as_sdf {
        builder.make_sdf();
    }
    builder.sprites(sprites.into_iter().collect());

    // TODO: decide if this is needed and/or configurable
    // builder.make_unique();

    builder
        .generate()
        .ok_or(SpriteError::UnableToGenerateSpritesheet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sprites() {
        let mut sprites = SpriteSources::default();
        sprites.add_source(
            "src1".to_string(),
            PathBuf::from("../tests/fixtures/sprites/src1"),
        ).unwrap();
        sprites.add_source(
            "src2".to_string(),
            PathBuf::from("../tests/fixtures/sprites/src2"),
        ).unwrap();

        assert_eq!(sprites.0.len(), 2);

        for generate_sdf in [true, false] {
            let paths = sprites
                .0
                .iter()
                .map(|v| v.value().clone())
                .collect::<Vec<_>>();
            test_src(paths.iter(), 1, "all_1", generate_sdf).await;
            test_src(paths.iter(), 2, "all_2", generate_sdf).await;

            let src1_path = sprites.get("src1").into_iter().collect::<Vec<_>>();
            test_src(src1_path.iter(), 1, "src1_1", generate_sdf).await;
            test_src(src1_path.iter(), 2, "src1_2", generate_sdf).await;

            let src2_path = sprites.get("src2").into_iter().collect::<Vec<_>>();
            test_src(src2_path.iter(), 1, "src2_1", generate_sdf).await;
            test_src(src2_path.iter(), 2, "src2_2", generate_sdf).await;
        }
    }

    async fn test_src(
        sources: impl Iterator<Item = &SpriteSource>,
        pixel_ratio: u8,
        filename: &str,
        generate_sdf: bool,
    ) {
        let sprites = get_spritesheet(sources, pixel_ratio, generate_sdf)
            .await
            .unwrap();
        let filename = if generate_sdf {
            format!("{filename}_sdf")
        } else {
            filename.to_string()
        };
        insta::assert_json_snapshot!(format!("{filename}.json"), sprites.get_index());
        let png = sprites.encode_png().unwrap();
        insta::assert_binary_snapshot!(&format!("{filename}.png"), png);
    }
}

#[tokio::test]
async fn test_directory_not_found() {
    let mut sprites = SpriteSources::default();
    let result = sprites.add_source("nothere".to_string(), PathBuf::from("/path/to/nowhere"));
    assert!(matches!(result, Err(SpriteError::DirectoryNotFound(..))));
}

#[tokio::test]
async fn test_not_a_directory() {
    let mut sprites = SpriteSources::default();
    let result = sprites.add_source(
        "notadir".to_string(),
        PathBuf::from("../tests/fixtures/sprites/notsrc2/ferris.png"),
    );
    assert!(matches!(result, Err(SpriteError::NotADirectory(..))));
}

#[tokio::test]
async fn test_empty_directory() {
    let sprites = SpriteSources::default();
    let result = sprites
        .validate_source_directory(&PathBuf::from("../tests/fixtures/sprites/notsrc1"))
        .await;
    assert!(matches!(result, Err(SpriteError::EmptyDirectory(..))));
}

#[tokio::test]
async fn test_sprite_source_scan() {
    use crate::sprites::SpriteSources;
    let result =
        SpriteSources::scan_directory_files(&PathBuf::from("../tests/fixtures/sprites/notsrc2"))
            .await;
    assert_eq!(result.as_ref().unwrap().0, 2);
    assert_eq!(result.unwrap().1, 0);
}

#[tokio::test]
async fn test_empty_file() {
    use crate::sprites::SpriteSources;
    let sprites = SpriteSources::default();
    let result = SpriteSources::validate_svg_file(
        &sprites,
        &PathBuf::from("../tests/fixtures/sprites/notsrc2/notasprite.txt"),
    )
    .await;
    assert!(matches!(result, Err(SpriteError::EmptyFile(..))));
}
