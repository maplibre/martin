use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt::Debug;
use std::path::PathBuf;

use actix_web::error::ErrorNotFound;
use futures::future::try_join_all;
use log::{info, warn};
use spreet::fs::get_svg_input_paths;
use spreet::resvg::tiny_skia::Pixmap;
use spreet::resvg::usvg::{Error as ResvgError, Options, Tree, TreeParsing};
use spreet::sprite::{generate_pixmap_from_svg, sprite_name, Spritesheet, SpritesheetBuilder};
use tokio::io::AsyncReadExt;

use crate::file_config::{FileConfigEnum, FileError};

#[derive(thiserror::Error, Debug)]
pub enum SpriteError {
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
    SpriteProcessingError(spreet::error::Error, PathBuf),

    #[error("{0} in file {}", .1.display())]
    SpriteParsingError(ResvgError, PathBuf),

    #[error("Unable to generate spritesheet")]
    UnableToGenerateSpritesheet,
}

pub fn resolve_sprites(config: &mut FileConfigEnum) -> Result<SpriteSources, FileError> {
    let cfg = config.extract_file_config();
    let mut results = SpriteSources::default();
    let mut directories = Vec::new();
    let mut configs = HashMap::new();

    if let Some(sources) = cfg.sources {
        for (id, source) in sources {
            configs.insert(id.clone(), source.clone());
            add_source(id, source.abs_path()?, &mut results);
        }
    };

    if let Some(paths) = cfg.paths {
        for path in paths {
            let Some(name) = path.file_name() else {
                warn!("Ignoring sprite source with no name from {}", path.display());
                continue;
            };
            directories.push(path.clone());
            add_source(name.to_string_lossy().to_string(), path, &mut results);
        }
    }

    *config = FileConfigEnum::from_configs(directories, configs, cfg.unrecognized);

    Ok(results)
}

fn add_source(id: String, path: PathBuf, results: &mut SpriteSources) {
    let disp_path = path.display();
    if path.is_file() {
        warn!("Ignoring non-directory sprite source {id} from {disp_path}");
    } else {
        match results.0.entry(id) {
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

#[derive(Debug, Clone, Default)]
pub struct SpriteSources(HashMap<String, SpriteSource>);

impl SpriteSources {
    pub fn get_sprite_source(&self, id: &str) -> actix_web::Result<&SpriteSource> {
        self.0
            .get(id)
            .ok_or_else(|| ErrorNotFound(format!("Sprite {id} does not exist")))
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
) -> Result<(String, Pixmap), SpriteError> {
    let on_err = |e| SpriteError::IoError(e, path.clone());

    let mut file = tokio::fs::File::open(&path).await.map_err(on_err)?;

    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).await.map_err(on_err)?;

    let tree = Tree::from_data(&buffer, &Options::default())
        .map_err(|e| SpriteError::SpriteParsingError(e, path.clone()))?;

    let pixmap = generate_pixmap_from_svg(&tree, pixel_ratio)
        .ok_or_else(|| SpriteError::UnableToReadSprite(path.clone()))?;

    Ok((name, pixmap))
}

pub async fn get_spritesheet(
    sources: impl Iterator<Item = &SpriteSource>,
    pixel_ratio: u8,
) -> Result<Spritesheet, SpriteError> {
    // Asynchronously load all SVG files from the given sources
    let sprites = try_join_all(sources.flat_map(|source| {
        get_svg_input_paths(&source.path, true)
            .into_iter()
            .map(|svg_path| {
                let name = sprite_name(&svg_path, &source.path);
                parse_sprite(name, svg_path, pixel_ratio)
            })
            .collect::<Vec<_>>()
    }))
    .await?;

    let mut builder = SpritesheetBuilder::new();
    builder
        .sprites(sprites.into_iter().collect())
        .pixel_ratio(pixel_ratio);

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
    use crate::file_config::FileConfig;
    use crate::OneOrMany::Many;

    #[actix_rt::test]
    async fn test_sprites() {
        let config = FileConfig {
            paths: Some(Many(vec![
                PathBuf::from("../tests/fixtures/sprites/src1"),
                PathBuf::from("../tests/fixtures/sprites/src2"),
            ])),
            ..FileConfig::default()
        };

        let sprites = resolve_sprites(&mut FileConfigEnum::Config(config))
            .unwrap()
            .0;
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
