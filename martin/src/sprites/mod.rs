use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::fmt::Debug;
use std::path::PathBuf;

use async_trait::async_trait;
use futures::future::try_join_all;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use spreet::resvg::usvg::{Error as ResvgError, Options, Tree, TreeParsing};
use spreet::{
    get_svg_input_paths, sprite_name, SpreetError, Sprite, Spritesheet, SpritesheetBuilder,
};
use tokio::io::AsyncReadExt;

use self::SpriteError::{SpriteInstError, SpriteParsingError, SpriteProcessingError};
use crate::config::UnrecognizedValues;
use crate::file_config::{ConfigExtras, FileConfigEnum, FileResult};

pub type SpriteResult<T> = Result<T, SpriteError>;

#[derive(thiserror::Error, Debug)]
pub enum SpriteError {
    #[error("Sprite {0} not found")]
    SpriteNotFound(String),

    #[error("IO error {0}: {}", .1.display())]
    IoError(std::io::Error, PathBuf),

    #[error("Sprite path is not a file: {}", .0.display())]
    InvalidFilePath(PathBuf),

    #[error("Sprite {0} uses bad file {}", .1.display())]
    InvalidSpriteFilePath(String, PathBuf),

    #[error("No sprite files found in {}", .0.display())]
    NoSpriteFilesFound(PathBuf),

    #[error("Sprite {} could not be loaded", .0.display())]
    UnableToReadSprite(PathBuf),

    #[error("{0} in file {}", .1.display())]
    SpriteProcessingError(SpreetError, PathBuf),

    #[error("{0} in file {}", .1.display())]
    SpriteParsingError(ResvgError, PathBuf),

    #[error("Unable to generate spritesheet")]
    UnableToGenerateSpritesheet,

    #[error("Unable to create a sprite from file {}", .0.display())]
    SpriteInstError(PathBuf),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct CatalogSpriteEntry {
    pub images: Vec<String>,
}

pub type SpriteCatalog = BTreeMap<String, CatalogSpriteEntry>;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpriteConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

#[async_trait]
impl ConfigExtras for SpriteConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

#[derive(Debug, Clone, Default)]
pub struct SpriteSources(HashMap<String, SpriteSource>);

impl SpriteSources {
    pub fn resolve(config: &mut FileConfigEnum<SpriteConfig>) -> FileResult<Self> {
        let Some(cfg) = config.extract_file_config()? else {
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
        };

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

    pub fn get_catalog(&self) -> SpriteResult<SpriteCatalog> {
        // TODO: all sprite generation should be pre-cached
        let mut entries = SpriteCatalog::new();
        for (id, source) in &self.0 {
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
            entries.insert(id.clone(), CatalogSpriteEntry { images });
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
                    warn!("Ignoring duplicate sprite source {} from {disp_path} because it was already configured for {}",
                    v.key(), v.get().path.display());
                }
                Entry::Vacant(v) => {
                    info!("Configured sprite source {} from {disp_path}", v.key());
                    v.insert(SpriteSource { path });
                }
            }
        };
    }

    /// Given a list of IDs in a format "id1,id2,id3", return a spritesheet with them all.
    /// `ids` may optionally end with "@2x" to request a high-DPI spritesheet.
    pub async fn get_sprites(&self, ids: &str) -> SpriteResult<Spritesheet> {
        let (ids, dpi) = if let Some(ids) = ids.strip_suffix("@2x") {
            (ids, 2)
        } else {
            (ids, 1)
        };

        let sprite_ids = ids
            .split(',')
            .map(|id| {
                self.0
                    .get(id)
                    .ok_or_else(|| SpriteError::SpriteNotFound(id.to_string()))
            })
            .collect::<SpriteResult<Vec<_>>>()?;

        get_spritesheet(sprite_ids.into_iter(), dpi).await
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
) -> SpriteResult<(String, Sprite)> {
    let on_err = |e| SpriteError::IoError(e, path.clone());

    let mut file = tokio::fs::File::open(&path).await.map_err(on_err)?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await.map_err(on_err)?;

    let tree = Tree::from_data(&buffer, &Options::default())
        .map_err(|e| SpriteParsingError(e, path.clone()))?;

    let sprite = Sprite::new(tree, pixel_ratio).ok_or_else(|| SpriteInstError(path.clone()))?;

    Ok((name, sprite))
}

pub async fn get_spritesheet(
    sources: impl Iterator<Item = &SpriteSource>,
    pixel_ratio: u8,
) -> SpriteResult<Spritesheet> {
    // Asynchronously load all SVG files from the given sources
    let mut futures = Vec::new();
    for source in sources {
        let paths = get_svg_input_paths(&source.path, true)
            .map_err(|e| SpriteProcessingError(e, source.path.clone()))?;
        for path in paths {
            let name = sprite_name(&path, &source.path)
                .map_err(|e| SpriteProcessingError(e, source.path.clone()))?;
            futures.push(parse_sprite(name, path, pixel_ratio));
        }
    }
    let sprites = try_join_all(futures).await?;
    let mut builder = SpritesheetBuilder::new();
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

        test_src(sprites.values(), 1, "all_1").await;
        test_src(sprites.values(), 2, "all_2").await;

        test_src(sprites.get("src1").into_iter(), 1, "src1_1").await;
        test_src(sprites.get("src1").into_iter(), 2, "src1_2").await;

        test_src(sprites.get("src2").into_iter(), 1, "src2_1").await;
        test_src(sprites.get("src2").into_iter(), 2, "src2_2").await;
    }

    async fn test_src(
        sources: impl Iterator<Item = &SpriteSource>,
        pixel_ratio: u8,
        filename: &str,
    ) {
        let path = PathBuf::from(format!("../tests/fixtures/sprites/expected/{filename}"));

        let sprites = get_spritesheet(sources, pixel_ratio).await.unwrap();
        let mut json = serde_json::to_string_pretty(sprites.get_index()).unwrap();
        json.push('\n');
        let png = sprites.encode_png().unwrap();

        #[cfg(feature = "bless-tests")]
        {
            use std::io::Write as _;
            let mut file = std::fs::File::create(path.with_extension("json")).unwrap();
            file.write_all(json.as_bytes()).unwrap();

            let mut file = std::fs::File::create(path.with_extension("png")).unwrap();
            file.write_all(&png).unwrap();
        }

        #[cfg(not(feature = "bless-tests"))]
        {
            let expected = std::fs::read_to_string(path.with_extension("json"))
                .expect("Unable to open expected JSON file, make sure to bless tests with\n  cargo test --features bless-tests\n");

            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&json).unwrap(),
                serde_json::from_str::<serde_json::Value>(&expected).unwrap(),
                "Make sure to run bless if needed:\n  cargo test --features bless-tests\n\n{json}",
            );

            let expected = std::fs::read(path.with_extension("png"))
                .expect("Unable to open expected PNG file, make sure to bless tests with\n  cargo test --features bless-tests\n");

            assert_eq!(
                png, expected,
                "Make sure to run bless if needed:\n  cargo test --features bless-tests\n\n{json}",
            );
        }
    }
}
