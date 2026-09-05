//! Sprite processing and serving for map tile rendering.
//!
//! Generates spritesheets from SVG files with support for high-DPI (@2x) and
//! SDF (Signed Distance Field) sprites for dynamic styling.
//!
//! # Usage
//!
//! ```rust,no_run
//! # async fn foo() {
//! use std::path::PathBuf;
//!
//! use martin_core::sprites::SpriteSources;
//!
//! let mut sources = SpriteSources::default();
//! sources.add_source("icons".to_string(), PathBuf::from("/path/to/svg/directory"));
//! let spritesheet = sources.get_sprites("icons@2x", false).await.unwrap();
//! # }
//! ```

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use dashmap::{DashMap, Entry};
use futures::stream::{self, StreamExt as _, TryStreamExt as _};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
pub use spreet::Spritesheet;
use spreet::resvg::usvg::{Options, Tree};
use spreet::{Sprite, SpritesheetBuilder, sprite_name as spreet_sprite_name};
use tokio::io::AsyncReadExt as _;
use tracing::{info, instrument, warn};

use self::SpriteError::{IoError, SpriteInstError, SpriteParsingError, SpriteProcessingError};
use crate::walk_files;

const SVG_EXTENSIONS: &[&str] = &["svg"];

/// Maximum number of distinct sprite source ids accepted in a single request.
///
/// Bounds the amount of per-request work (file reads, SVG parses, rasterization)
/// an attacker can trigger by stuffing a request path with many ids.
const MAX_SPRITE_IDS_PER_REQUEST: usize = 128;

/// Maximum number of SVGs parsed and rasterized concurrently while building one spritesheet.
const MAX_CONCURRENT_SPRITE_PARSES: usize = 16;

fn discover_svgs(path: &Path) -> Result<Vec<PathBuf>, SpriteError> {
    walk_files(path, SVG_EXTENSIONS).map_err(|e| IoError(e.into(), path.to_path_buf()))
}

/// `spreet::sprite_name` joins a nested sprite's directory onto its file stem with the
/// platform separator; the name is a URL path segment, so keep it `/`-joined everywhere.
fn sprite_name(path: &Path, base_path: &Path) -> spreet::SpreetResult<String> {
    spreet_sprite_name(path, base_path).map(|name| name.replace(std::path::MAIN_SEPARATOR, "/"))
}

/// Splits a comma-separated, optionally `@2x`-suffixed sprite id list from a
/// request path into the requested pixel ratio and a deduplicated, sorted
/// list of source ids.
///
/// Sorting and deduplicating here (rather than only at spritesheet-build
/// time) means equivalent requests - regardless of id order or repeat count
/// - do the same amount of work and can share one cache entry.
fn split_and_dedup_ids(ids: &str) -> (Vec<&str>, u8) {
    let (ids, dpi) = if let Some(ids) = ids.strip_suffix("@2x") {
        (ids, 2)
    } else {
        (ids, 1)
    };

    let mut unique_ids: Vec<&str> = ids.split(',').collect();
    unique_ids.sort_unstable();
    unique_ids.dedup();

    (unique_ids, dpi)
}

/// Normalizes a request-path sprite id list so that equivalent requests
/// (different id order, repeated ids) share one cache entry instead of
/// each producing a distinct cache miss.
#[must_use]
pub fn normalize_sprite_ids(ids: &str) -> String {
    let (unique_ids, dpi) = split_and_dedup_ids(ids);
    join_ids(&unique_ids, dpi)
}

/// Joins sprite ids back into request-path form, with the `@2x` suffix for the high-DPI ratio.
fn join_ids<S: AsRef<str>>(ids: &[S], dpi: u8) -> String {
    let joined = ids.iter().map(AsRef::as_ref).collect::<Vec<_>>().join(",");
    if dpi == 2 {
        format!("{joined}@2x")
    } else {
        joined
    }
}

mod error;
pub use error::SpriteError;

mod cache;
pub use cache::{NO_SPRITE_CACHE, OptSpriteCache, SpriteCache, SpriteCacheKey};

/// Sprite source metadata.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(
    feature = "unstable-schemas",
    derive(schemars::JsonSchema, utoipa::ToSchema)
)]
pub struct CatalogSpriteEntry {
    /// Available sprite image names.
    pub images: Vec<String>,
    /// Total size of the spritesheet in bytes.
    // utoipa 5.4 has no `PartialSchema` impl for `NonZeroUsize`, so describe
    // the field as a positive integer for the OpenAPI side; serde still
    // serializes the inner usize.
    #[cfg_attr(feature = "unstable-schemas", schema(value_type = u64, minimum = 1))]
    pub size_in_bytes: Option<NonZeroUsize>,
    /// Timestamp of the spritesheet's last modification.
    pub last_modified_at: Option<DateTime<Utc>>,
}

/// Catalog mapping sprite names to metadata (e.g., "icons" -> [`CatalogSpriteEntry`]).
pub type SpriteCatalog = BTreeMap<String, CatalogSpriteEntry>;

/// Thread-safe sprite source manager for serving sprites as `.png` or `.json`.
#[derive(Debug, Clone, Default)]
pub struct SpriteSources {
    /// Map of sprite source id to its directory.
    sources: DashMap<String, SpriteSource>,
    /// Map of alias name to the sprite source ids it combines.
    aliases: DashMap<String, Vec<String>>,
}

impl SpriteSources {
    /// Returns a catalog of all sprite sources and aliases.
    ///
    /// An alias is listed under its own name with the images of every source it combines.
    pub fn get_catalog(&self) -> Result<SpriteCatalog, SpriteError> {
        // TODO: all sprite generation should be pre-cached
        let mut entries = SpriteCatalog::new();
        for source in &self.sources {
            let paths = discover_svgs(&source.path)?;
            let mut images = Vec::with_capacity(paths.len());
            for path in paths {
                images.push(
                    sprite_name(&path, &source.path)
                        .map_err(|e| SpriteProcessingError(e, source.path.clone()))?,
                );
            }
            images.sort();
            entries.insert(
                source.key().clone(),
                CatalogSpriteEntry {
                    images,
                    // FIXME: render once and report the encoded PNG byte count.
                    size_in_bytes: None,
                    // FIXME: stat the SVG inputs and surface their newest mtime.
                    last_modified_at: None,
                },
            );
        }
        let aliases = self
            .aliases
            .iter()
            .map(|alias| {
                let mut images: Vec<String> = alias
                    .value()
                    .iter()
                    .filter_map(|source| entries.get(source))
                    .flat_map(|entry| entry.images.iter().cloned())
                    .collect();
                images.sort();
                images.dedup();
                (
                    alias.key().clone(),
                    CatalogSpriteEntry {
                        images,
                        size_in_bytes: None,
                        last_modified_at: None,
                    },
                )
            })
            .collect::<Vec<_>>();
        entries.extend(aliases);
        Ok(entries)
    }

    /// Registers a named combination of sprite sources that serves like a single source.
    ///
    /// Every member must name a configured sprite source, not another alias.
    /// An alias may share the name of a source it references.
    /// Requests for such a name serve the alias.
    pub fn add_alias(&self, name: String, sprites: Vec<String>) -> Result<(), SpriteError> {
        if name.is_empty() || name.contains(',') || name.ends_with("@2x") {
            return Err(SpriteError::InvalidAliasName(name));
        }
        if sprites.is_empty() {
            return Err(SpriteError::EmptyAlias(name));
        }
        if sprites.len() > MAX_SPRITE_IDS_PER_REQUEST {
            return Err(SpriteError::TooManySpritesInAlias {
                alias: name,
                requested: sprites.len(),
                max: MAX_SPRITE_IDS_PER_REQUEST,
            });
        }
        for sprite in &sprites {
            if self.aliases.contains_key(sprite) {
                return Err(SpriteError::AliasWithinAlias {
                    alias: name,
                    sprite: sprite.clone(),
                });
            }
            if !self.sources.contains_key(sprite) {
                return Err(SpriteError::AliasSpriteNotFound {
                    alias: name,
                    sprite: sprite.clone(),
                });
            }
        }
        if self.sources.contains_key(&name) {
            info!(
                sprite.alias = %name,
                "Sprite alias shadows a sprite source of the same name; requests for it will serve the alias"
            );
        }
        info!(
            sprite.alias = %name,
            source.ids = %sprites.join(", "),
            "Configured sprite alias"
        );
        self.aliases.insert(name, sprites);
        Ok(())
    }

    /// Splits a request id list, replaces every alias with its member sources, and returns
    /// the sorted, deduplicated ids with the requested pixel ratio.
    fn expanded_ids(&self, ids: &str) -> (Vec<String>, u8) {
        let (unique_ids, dpi) = split_and_dedup_ids(ids);
        let mut expanded: Vec<String> = Vec::with_capacity(unique_ids.len());
        for id in unique_ids {
            if let Some(alias) = self.aliases.get(id) {
                expanded.extend(alias.value().iter().cloned());
            } else {
                expanded.push(id.to_owned());
            }
        }
        expanded.sort_unstable();
        expanded.dedup();
        (expanded, dpi)
    }

    /// Expands every alias in a request id list into its member sources, normalized like
    /// [`normalize_sprite_ids`].
    ///
    /// Callers that need the sources a request actually resolves to use this,
    /// e.g. to build cache keys that can be invalidated per source.
    #[must_use]
    pub fn expand_sprite_ids(&self, ids: &str) -> String {
        if self.aliases.is_empty() {
            return ids.to_owned();
        }
        let (expanded, dpi) = self.expanded_ids(ids);
        join_ids(&expanded, dpi)
    }

    /// Adds a sprite source directory containing SVG files.
    /// Files are ignored - only directories accepted. Duplicates ignored with warning.
    pub fn add_source(&self, id: String, path: PathBuf) {
        let disp_path = path.display();
        if path.is_file() {
            warn!(
                source.id = %id,
                sprite.path = %disp_path,
                "Ignoring non-directory sprite source"
            );
        } else {
            match self.sources.entry(id) {
                Entry::Occupied(v) => {
                    warn!(
                        source.id = %v.key(),
                        sprite.path.kept = %v.get().path.display(),
                        sprite.path.dropped = %disp_path,
                        "Ignoring duplicate sprite source: already configured for another path"
                    );
                }
                Entry::Vacant(v) => {
                    info!(
                        source.id = %v.key(),
                        sprite.path = %disp_path,
                        "Configured sprite source"
                    );
                    if let Ok(paths) = discover_svgs(&path)
                        && paths.is_empty()
                    {
                        warn!(
                            source.id = %v.key(),
                            sprite.path = %disp_path,
                            "No sprite SVG files found in directory to generate spritesheets from. \
                             Sprite requests for this source will fail, until at least one svg is present."
                        );
                    }
                    v.insert(SpriteSource { path });
                }
            }
        }
    }

    /// Generates a spritesheet from comma-separated sprite source IDs.
    ///
    /// Ids may name aliases registered via [`Self::add_alias`], which expand to their member sources.
    /// Append "@2x" for high-DPI sprites.
    /// Set `as_sdf` for SDF sprites.
    #[instrument(
        level = "debug",
        skip(self),
        fields(source.ids = %ids, sprite.sdf = as_sdf),
        err(Debug),
    )]
    pub async fn get_sprites(&self, ids: &str, as_sdf: bool) -> Result<Spritesheet, SpriteError> {
        let (unique_ids, dpi) = self.expanded_ids(ids);

        if unique_ids.len() > MAX_SPRITE_IDS_PER_REQUEST {
            return Err(SpriteError::TooManySpriteIds {
                requested: unique_ids.len(),
                max: MAX_SPRITE_IDS_PER_REQUEST,
            });
        }

        let sprite_ids = unique_ids
            .iter()
            .map(|id| self.get(id))
            .collect::<Result<Vec<_>, SpriteError>>()?;

        get_spritesheet(sprite_ids.iter(), dpi, as_sdf).await
    }

    fn get(&self, id: &str) -> Result<SpriteSource, SpriteError> {
        match self.sources.get(id) {
            Some(v) => Ok(v.clone()),
            None => Err(SpriteError::SpriteNotFound(id.to_owned())),
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
    let on_err = |e| IoError(e, path.clone());

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
#[instrument(
    level = "debug",
    skip_all,
    fields(sprite.pixel_ratio = pixel_ratio, sprite.sdf = as_sdf),
    err(Debug),
)]
pub async fn get_spritesheet(
    sources: impl Iterator<Item = &SpriteSource>,
    pixel_ratio: u8,
    as_sdf: bool,
) -> Result<Spritesheet, SpriteError> {
    // Asynchronously load all SVG files from the given sources
    let mut futures = Vec::new();
    for source in sources {
        let paths = discover_svgs(&source.path)?;
        // SpritesheetBuilder::generate will return None if the folder does not contain any SVGs
        if paths.is_empty() {
            return Err(SpriteError::NoSpriteFilesFound(source.path.clone()));
        }
        for path in paths {
            let name = sprite_name(&path, &source.path)
                .map_err(|e| SpriteProcessingError(e, source.path.clone()))?;
            futures.push(parse_sprite(name, path, pixel_ratio, as_sdf));
        }
    }
    let sprites: Vec<(String, Sprite)> = stream::iter(futures)
        .buffer_unordered(MAX_CONCURRENT_SPRITE_PARSES)
        .try_collect()
        .await?;
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
    use std::assert_matches;

    use super::*;

    #[test]
    fn normalize_sprite_ids_collapses_order_and_duplicates() {
        assert_eq!(normalize_sprite_ids("b,a,b"), "a,b");
        assert_eq!(normalize_sprite_ids("b,a@2x"), "a,b@2x");
        assert_eq!(normalize_sprite_ids("a"), "a");
    }

    #[tokio::test]
    async fn duplicate_ids_are_deduplicated_before_rendering() {
        let sprites = SpriteSources::default();
        sprites.add_source("src1".to_owned(), PathBuf::from("../tests/fixtures/sprites/src1"));

        let single = sprites.get_sprites("src1", false).await.unwrap();
        let repeated_ids = vec!["src1"; 50].join(",");
        let repeated = sprites.get_sprites(&repeated_ids, false).await.unwrap();

        assert_eq!(
            serde_json::to_value(single.get_index()).unwrap(),
            serde_json::to_value(repeated.get_index()).unwrap()
        );
        assert_eq!(single.encode_png().unwrap(), repeated.encode_png().unwrap());
    }

    fn two_sources() -> SpriteSources {
        let sprites = SpriteSources::default();
        sprites.add_source("src1".to_owned(), PathBuf::from("../tests/fixtures/sprites/src1"));
        sprites.add_source("src2".to_owned(), PathBuf::from("../tests/fixtures/sprites/src2"));
        sprites
    }

    #[tokio::test]
    async fn an_alias_serves_the_same_sheet_as_the_explicit_composite() {
        let sprites = two_sources();
        sprites
            .add_alias("icons".to_owned(), vec!["src1".to_owned(), "src2".to_owned()])
            .unwrap();

        assert_eq!(sprites.expand_sprite_ids("icons"), "src1,src2");
        assert_eq!(sprites.expand_sprite_ids("icons@2x"), "src1,src2@2x");
        assert_eq!(
            sprites.expand_sprite_ids("src2,icons"),
            "src1,src2",
            "a source shared between the request and the alias appears once"
        );
        let aliased = sprites.get_sprites("icons", false).await.unwrap();
        let explicit = sprites.get_sprites("src1,src2", false).await.unwrap();
        assert_eq!(
            serde_json::to_value(aliased.get_index()).unwrap(),
            serde_json::to_value(explicit.get_index()).unwrap()
        );
        assert_eq!(aliased.encode_png().unwrap(), explicit.encode_png().unwrap());
    }

    #[test]
    fn invalid_aliases_are_rejected() {
        let sprites = two_sources();

        for name in ["", "has,comma", "icons@2x"] {
            let err = sprites
                .add_alias(name.to_owned(), vec!["src1".to_owned()])
                .unwrap_err();
            assert_matches!(err, SpriteError::InvalidAliasName(_), "{name:?}");
        }

        let err = sprites.add_alias("empty".to_owned(), vec![]).unwrap_err();
        assert_matches!(err, SpriteError::EmptyAlias(_));

        let err = sprites
            .add_alias("unknown".to_owned(), vec!["nonexistent".to_owned()])
            .unwrap_err();
        assert_matches!(err, SpriteError::AliasSpriteNotFound { .. });

        sprites
            .add_alias("icons".to_owned(), vec!["src1".to_owned()])
            .unwrap();
        let err = sprites
            .add_alias("nested".to_owned(), vec!["icons".to_owned()])
            .unwrap_err();
        assert_matches!(err, SpriteError::AliasWithinAlias { .. });

        let too_many = vec!["src1".to_owned(); MAX_SPRITE_IDS_PER_REQUEST + 1];
        let err = sprites.add_alias("big".to_owned(), too_many).unwrap_err();
        assert_matches!(err, SpriteError::TooManySpritesInAlias { .. });
    }

    #[test]
    fn the_catalog_lists_aliases_with_merged_images() {
        let sprites = two_sources();
        sprites
            .add_alias("icons".to_owned(), vec!["src1".to_owned(), "src2".to_owned()])
            .unwrap();

        let catalog = sprites.get_catalog().unwrap();
        let entry = catalog.get("icons").expect("alias is cataloged");
        assert_eq!(entry.images, ["another_bicycle", "bear", "bicycle", "sub/circle"]);
    }

    #[tokio::test]
    async fn too_many_ids_are_rejected_before_any_work() {
        let sprites = SpriteSources::default();
        sprites.add_source("src1".to_owned(), PathBuf::from("../tests/fixtures/sprites/src1"));

        let ids = (0..=MAX_SPRITE_IDS_PER_REQUEST)
            .map(|i| format!("nonexistent{i}"))
            .collect::<Vec<_>>()
            .join(",");

        let Err(err) = sprites.get_sprites(&ids, false).await else {
            panic!("expected TooManySpriteIds, got Ok");
        };
        assert_matches!(err, SpriteError::TooManySpriteIds { .. });
    }

    #[tokio::test]
    async fn exactly_max_ids_is_not_rejected_by_the_count_check() {
        let sprites = SpriteSources::default();

        let ids = (0..MAX_SPRITE_IDS_PER_REQUEST)
            .map(|i| format!("nonexistent{i}"))
            .collect::<Vec<_>>()
            .join(",");

        let Err(err) = sprites.get_sprites(&ids, false).await else {
            panic!("expected SpriteNotFound, got Ok");
        };
        assert_matches!(err, SpriteError::SpriteNotFound(_));
    }

    #[tokio::test]
    async fn sprites() {
        let sprites = SpriteSources::default();
        sprites.add_source("src1".to_owned(), PathBuf::from("../tests/fixtures/sprites/src1"));
        sprites.add_source("src2".to_owned(), PathBuf::from("../tests/fixtures/sprites/src2"));

        assert_eq!(sprites.sources.len(), 2);

        for generate_sdf in [true, false] {
            let paths = sprites
                .sources
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

    #[cfg(unix)]
    #[test]
    fn k8s_configmap_symlinks_yield_clean_sprite_names() {
        use std::fs::{create_dir_all, write};
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let real_dir = root.join("..2024_05_17_17_57_51.390489675");
        create_dir_all(&real_dir).unwrap();
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>";
        write(real_dir.join("foo.svg"), svg).unwrap();
        write(real_dir.join("bar.svg"), svg).unwrap();
        symlink("..2024_05_17_17_57_51.390489675", root.join("..data")).unwrap();
        symlink("..data/foo.svg", root.join("foo.svg")).unwrap();
        symlink("..data/bar.svg", root.join("bar.svg")).unwrap();

        let sprites = SpriteSources::default();
        sprites.add_source("foobar".to_owned(), root.to_path_buf());

        let catalog = sprites.get_catalog().expect("catalog");
        let entry = catalog.get("foobar").expect("foobar source registered");
        assert_eq!(
            entry.images,
            vec!["bar".to_owned(), "foo".to_owned()],
            "expected plain sprite names without dotfile directory prefixes"
        );
    }

    #[test]
    fn a_nested_sprite_name_is_forward_slash_joined_on_every_platform() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let sub = tmp.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"1\" height=\"1\"/>";
        std::fs::write(sub.join("circle.svg"), svg).unwrap();

        let sprites = SpriteSources::default();
        sprites.add_source("nested".to_owned(), tmp.path().to_path_buf());

        let catalog = sprites.get_catalog().expect("catalog");
        let entry = catalog.get("nested").expect("nested source registered");
        assert_eq!(
            entry.images,
            vec!["sub/circle".to_owned()],
            "nested sprite names must use `/` even on windows, since they are URL path segments"
        );
    }

    #[tokio::test]
    async fn directory_without_svgs_yields_helpful_error() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(tmp.path().join("sprite.json"), b"{}").unwrap();
        std::fs::write(tmp.path().join("sprite.png"), b"\x89PNG\r\n").unwrap();

        let sprites = SpriteSources::default();
        sprites.add_source("bad".to_owned(), tmp.path().to_path_buf());

        let source = sprites.get("bad").expect("source registered");
        let Err(err) = get_spritesheet([source].iter(), 1, false).await else {
            panic!("expected NoSpriteFilesFound, got Ok");
        };

        assert_matches!(err, SpriteError::NoSpriteFilesFound(_));
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
            filename.to_owned()
        };
        insta::assert_json_snapshot!(format!("{filename}.json"), sprites.get_index());
        let png = sprites.encode_png().unwrap();
        insta::assert_binary_snapshot!(&format!("{filename}.png"), png);
    }
}
