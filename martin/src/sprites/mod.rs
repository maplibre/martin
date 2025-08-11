use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::path::PathBuf;

use dashmap::{DashMap, Entry};
use futures::future::try_join_all;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use spreet::resvg::usvg::{Options, Tree};
use spreet::{Sprite, Spritesheet, SpritesheetBuilder, get_svg_input_paths, sprite_name};
use tokio::io::AsyncReadExt;

use self::SpriteError::{SpriteInstError, SpriteParsingError, SpriteProcessingError};
use crate::file_config::{FileConfigEnum, FileResult};

mod config;
pub use config::SpriteConfig;

mod error;
pub use error::SpriteError;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogSpriteEntry {
    pub images: Vec<String>,
}

pub type SpriteCatalog = HashMap<String, CatalogSpriteEntry>;

#[derive(Debug, Clone, Default)]
pub struct SpriteSources(DashMap<String, SpriteSource>);

impl SpriteSources {
    pub fn resolve(config: &mut FileConfigEnum<SpriteConfig>) -> FileResult<Self> {
        let Some(cfg) = config.extract_file_config(None)? else {
            return Ok(Self::default());
        };

        let mut results = Self::default();
        let mut directories = Vec::new();
        let mut configs = BTreeMap::new();

        if let Some(sources) = cfg.sources {
            for (id, source) in sources {
                configs.insert(id.clone(), source.clone());
                results.add_source(id, source.abs_path()?);
            }
        }

        for path in cfg.paths {
            let Some(name) = path.file_name() else {
                warn!(
                    "Ignoring sprite source with no name from {}",
                    path.display()
                );
                continue;
            };
            directories.push(path.clone());
            results.add_source(name.to_string_lossy().to_string(), path);
        }

        *config = FileConfigEnum::new_extended(directories, configs, cfg.custom);

        Ok(results)
    }

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

    fn add_source(&mut self, id: String, path: PathBuf) {
        let disp_path = path.display();
        if path.is_file() {
            warn!("Ignoring non-directory sprite source {id} from {disp_path}");
        } else {
            match self.0.entry(id) {
                Entry::Occupied(v) => {
                    warn!(
                        "Ignoring duplicate sprite source {} from {disp_path} because it was already configured for {}",
                        v.key(),
                        v.get().path.display()
                    );
                }
                Entry::Vacant(v) => {
                    info!("Configured sprite source {} from {disp_path}", v.key());
                    v.insert(SpriteSource { path });
                }
            }
        }
    }

    /// Given a list of IDs in a format "id1,id2,id3", return a spritesheet with them all.
    /// `ids` may optionally end with "@2x" to request a high-DPI spritesheet.
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

#[derive(Clone, Debug)]
pub struct SpriteSource {
    path: PathBuf,
}

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
    use std::path::PathBuf;

    use super::*;

    #[actix_rt::test]
    async fn test_sprites() {
        let mut cfg = FileConfigEnum::new(vec![
            PathBuf::from("../tests/fixtures/sprites/src1"),
            PathBuf::from("../tests/fixtures/sprites/src2"),
        ]);

        let sprites = SpriteSources::resolve(&mut cfg).unwrap().0;
        assert_eq!(sprites.len(), 2);

        for generate_sdf in [true, false] {
            let paths = sprites
                .iter()
                .map(|v| v.value().clone())
                .collect::<Vec<_>>();
            test_src(paths.iter(), 1, "all_1", generate_sdf).await;
            test_src(paths.iter(), 2, "all_2", generate_sdf).await;

            let src1_path = sprites
                .get("src1")
                .into_iter()
                .map(|v| v.value().clone())
                .collect::<Vec<_>>();
            test_src(src1_path.iter(), 1, "src1_1", generate_sdf).await;
            test_src(src1_path.iter(), 2, "src1_2", generate_sdf).await;

            let src2_path = sprites
                .get("src2")
                .into_iter()
                .map(|v| v.value().clone())
                .collect::<Vec<_>>();
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
