//! Tile sources over a mosaic of many COGs, each covering one tile at the mosaic's file zoom.

use std::io;
use std::path::{Component, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use cog_tiles::{AsyncCogReader, CogInfo, FileBackend, ObjectStoreBackend, TileFormat};
use futures::TryStreamExt as _;
use martin_tile_utils::TileCoord;
use moka::future::Cache;
use object_store::ObjectStore;
use object_store::local::LocalFileSystem;
use object_store::path::Path;
use tilejson::tilejson;

use crate::CacheZoomRange;
use crate::tiles::cog::source::{CogSource, CogTiles, cog_coord, warn_skipped_images};
use crate::tiles::cog::{CogError, MosaicFileError, MosaicTemplateError};

/// How a mosaic of COGs is laid out and how many opened files it keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CogMosaicOptions {
    /// The zoom level at which each file covers exactly one tile, also the lowest zoom served.
    pub file_zoom: u8,
    /// The highest zoom served. `None` uses the highest zoom of the file opened at startup.
    pub max_zoom: Option<u8>,
    /// The most opened files (and known-missing ones) kept at once.
    pub max_cached_files: u64,
    /// How long an opened file is kept, so replaced or added files are picked up eventually.
    pub file_expiry: Duration,
    /// How long an opened file is kept without being used. `None` keeps it until `file_expiry`.
    pub file_idle_timeout: Option<Duration>,
}

impl CogSource {
    /// Creates a source over a mosaic of local COGs under `root`, named by `template`
    /// (e.g. `{quadkey}.tif` or `{z}/{x}/{y}.tif`) relative to it.
    ///
    /// One file under `root` is opened at startup to learn the tile size and format;
    /// the others are opened on first use.
    pub async fn new_mosaic_dir(
        id: String,
        root: PathBuf,
        template: &str,
        options: CogMosaicOptions,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let location = root.join(template).display().to_string();
        let lister =
            LocalFileSystem::new_with_prefix(&root).map_err(|source| CogError::MosaicListing {
                location: location.clone(),
                source,
            })?;
        let mosaic = Mosaic::new(template, &options, MosaicFiles::Dir(root))?;
        let info = mosaic.probe(&lister, &Path::default(), &location).await?;
        Self::from_mosaic(id, mosaic, &info, &options, location, cache_zoom)
    }

    /// Creates a source over a mosaic of COGs under `prefix` in `store`, named by `template`
    /// (e.g. `{quadkey}.tif` or `{z}/{x}/{y}.tif`) relative to it.
    ///
    /// `location` is the display form of the mosaic (e.g. a URL without credentials), used in errors.
    /// The first object under `prefix` whose key matches `template` is opened at startup to learn
    /// the tile size and format, which needs permission to list the prefix.
    /// The others are opened on first use.
    pub async fn new_mosaic_object_store(
        id: String,
        store: Arc<dyn ObjectStore>,
        prefix: Path,
        template: &str,
        location: String,
        options: CogMosaicOptions,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let files = MosaicFiles::ObjectStore {
            store: Arc::clone(&store),
            prefix: prefix.clone(),
        };
        let mosaic = Mosaic::new(template, &options, files)?;
        let info = mosaic.probe(&*store, &prefix, &location).await?;
        Self::from_mosaic(id, mosaic, &info, &options, location, cache_zoom)
    }

    fn from_mosaic(
        id: String,
        mosaic: Mosaic,
        info: &CogInfo,
        options: &CogMosaicOptions,
        location: String,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let tilejson = tilejson! {
            tiles: vec![],
            minzoom: options.file_zoom,
            maxzoom: options.max_zoom.unwrap_or(info.max_zoom),
        };
        Self::with_layout(
            id,
            CogTiles::Mosaic(Arc::new(mosaic)),
            tilejson,
            (info.tile_size, info.format),
            location,
            cache_zoom,
        )
    }
}

/// Serves tiles from many COGs, each covering exactly one tile at the mosaic's file zoom.
///
/// A request for `z/x/y` with `z >= file_zoom` goes to the file of its ancestor tile at
/// `file_zoom`, whose name comes from the [`KeyTemplate`]. Nothing is listed or indexed: files
/// are opened on first use and kept in a bounded, expiring cache, missing ones included.
#[derive(Debug)]
pub(super) struct Mosaic {
    template: KeyTemplate,
    file_zoom: u8,
    files: MosaicFiles,
    readers: Cache<String, Option<Arc<MosaicReader>>>,
    /// The tile size and format of the first file opened, which every other file must share.
    layout: OnceLock<(u32, TileFormat)>,
}

impl Mosaic {
    fn new(
        template: &str,
        options: &CogMosaicOptions,
        files: MosaicFiles,
    ) -> Result<Self, CogError> {
        if options.file_zoom > cog_tiles::MAX_ZOOM {
            return Err(CogError::FileZoomTooHigh {
                file_zoom: options.file_zoom,
            });
        }
        let key_template =
            KeyTemplate::new(template).map_err(|reason| CogError::InvalidTemplate {
                template: template.to_owned(),
                reason,
            })?;
        // Placeholders render digits only, so one rendered name shows where all of them lead.
        let sample = key_template.render(options.file_zoom, 0, 0);
        if !std::path::Path::new(&sample)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(CogError::InvalidTemplate {
                template: template.to_owned(),
                reason: MosaicTemplateError::NotRelative,
            });
        }
        let mut readers = Cache::builder()
            .max_capacity(options.max_cached_files)
            .time_to_live(options.file_expiry);
        if let Some(tti) = options.file_idle_timeout {
            readers = readers.time_to_idle(tti);
        }
        Ok(Self {
            template: key_template,
            file_zoom: options.file_zoom,
            files,
            readers: readers.build(),
            layout: OnceLock::new(),
        })
    }

    /// Reads one tile, or `None` if no file holds it or the file has no tile there.
    pub(super) async fn get_tile(&self, xyz: TileCoord) -> Result<Option<Bytes>, CogError> {
        match self.reader_for(xyz).await? {
            Some(reader) => reader.get_tile(cog_coord(xyz)?).await,
            None => Ok(None),
        }
    }

    /// Opens (or gets from the cache) the file that holds `xyz`.
    async fn reader_for(&self, xyz: TileCoord) -> Result<Option<Arc<MosaicReader>>, CogError> {
        let Some(shift) = xyz.z().checked_sub(self.file_zoom) else {
            return Ok(None);
        };
        let file_tile = (xyz.x() >> shift, xyz.y() >> shift);
        let key = self
            .template
            .render(self.file_zoom, file_tile.0, file_tile.1);
        let open = async {
            let Some(reader) = self.files.open(&key).await? else {
                return Ok(None);
            };
            warn_skipped_images(&key, reader.skipped_images());
            self.check(reader.info(), file_tile)?;
            Ok(Some(Arc::new(reader)))
        };
        self.readers
            .try_get_with(key.clone(), open)
            .await
            .map_err(|source| CogError::MosaicFile { key, source })
    }

    /// Checks that a file covers its key's tile and matches the rest of the mosaic.
    fn check(&self, info: &CogInfo, (x, y): (u32, u32)) -> Result<(), MosaicFileError> {
        let tile = cog_tiles::TileCoord::new(self.file_zoom, x, y)?;
        let [min_x, min_y, max_x, max_y] = tile.mercator_bounds();
        let [c_min_x, c_min_y, c_max_x, c_max_y] = info.mercator_bounds;
        let tolerance = (max_x - min_x) * 1e-6;
        if c_min_x > min_x + tolerance
            || c_min_y > min_y + tolerance
            || c_max_x < max_x - tolerance
            || c_max_y < max_y - tolerance
        {
            return Err(MosaicFileError::DoesNotCover { tile });
        }
        if info.max_zoom < self.file_zoom {
            return Err(MosaicFileError::BelowFileZoom {
                max_zoom: info.max_zoom,
                file_zoom: self.file_zoom,
            });
        }
        let layout = (info.tile_size, info.format);
        let (expected_tile_size, expected_format) = *self.layout.get_or_init(|| layout);
        if (expected_tile_size, expected_format) != layout {
            return Err(MosaicFileError::Layout {
                tile_size: info.tile_size,
                format: info.format,
                expected_tile_size,
                expected_format,
            });
        }
        Ok(())
    }

    /// Opens the first listed file under `prefix` that matches the template, to describe the mosaic.
    async fn probe(
        &self,
        lister: &dyn ObjectStore,
        prefix: &Path,
        location: &str,
    ) -> Result<CogInfo, CogError> {
        let listing_error = |source| CogError::MosaicListing {
            location: location.to_owned(),
            source,
        };
        let mut objects = lister.list(Some(prefix));
        while let Some(object) = objects.try_next().await.map_err(listing_error)? {
            let Some(parts) = object.location.prefix_match(prefix) else {
                continue;
            };
            let key = parts
                .map(|part| part.as_ref().to_owned())
                .collect::<Vec<_>>()
                .join("/");
            let Some((x, y)) = self.template.parse(&key, self.file_zoom) else {
                continue;
            };
            let xyz = TileCoord::new_unchecked(self.file_zoom, x, y);
            if let Some(reader) = self.reader_for(xyz).await? {
                return Ok(*reader.info());
            }
        }
        Err(CogError::MosaicEmpty {
            location: location.to_owned(),
            template: self.template.template.clone(),
        })
    }
}

/// Where a mosaic's files are.
#[derive(Debug)]
enum MosaicFiles {
    /// Under a local directory.
    Dir(PathBuf),
    /// Under a prefix in an object store.
    ObjectStore {
        store: Arc<dyn ObjectStore>,
        prefix: Path,
    },
}

impl MosaicFiles {
    /// Opens the file named `key`, or returns `None` if it does not exist.
    async fn open(&self, key: &str) -> Result<Option<MosaicReader>, MosaicFileError> {
        match self {
            Self::Dir(root) => match FileBackend::open(root.join(key)).await {
                Ok(backend) => Ok(Some(MosaicReader::File(
                    AsyncCogReader::try_from_backend(backend).await?,
                ))),
                Err(e) if is_not_found(&e) => Ok(None),
                Err(e) => Err(e.into()),
            },
            Self::ObjectStore { store, prefix } => {
                let path = if prefix.as_ref().is_empty() {
                    Path::from(key)
                } else {
                    Path::from(format!("{prefix}/{key}"))
                };
                match ObjectStoreBackend::open(Arc::clone(store), path).await {
                    Ok(backend) => Ok(Some(MosaicReader::ObjectStore(
                        AsyncCogReader::try_from_backend(backend).await?,
                    ))),
                    Err(e) if is_not_found(&e) => Ok(None),
                    Err(e) => Err(e.into()),
                }
            }
        }
    }
}

/// Whether opening a file failed only because it does not exist.
fn is_not_found(error: &cog_tiles::CogError) -> bool {
    let cog_tiles::CogError::Backend { source, .. } = error else {
        return false;
    };
    source
        .downcast_ref::<io::Error>()
        .is_some_and(|e| e.kind() == io::ErrorKind::NotFound)
        || matches!(
            source.downcast_ref::<object_store::Error>(),
            Some(object_store::Error::NotFound { .. })
        )
}

/// One opened file of a mosaic.
#[derive(Debug)]
enum MosaicReader {
    File(AsyncCogReader<FileBackend>),
    ObjectStore(AsyncCogReader<ObjectStoreBackend>),
}

impl MosaicReader {
    fn info(&self) -> &CogInfo {
        match self {
            Self::File(reader) => reader.info(),
            Self::ObjectStore(reader) => reader.info(),
        }
    }

    fn skipped_images(&self) -> &[cog_tiles::SkippedImage] {
        match self {
            Self::File(reader) => reader.skipped_images(),
            Self::ObjectStore(reader) => reader.skipped_images(),
        }
    }

    async fn get_tile(&self, coord: cog_tiles::TileCoord) -> Result<Option<Bytes>, CogError> {
        Ok(match self {
            Self::File(reader) => reader.get_tile(coord).await?,
            Self::ObjectStore(reader) => reader.get_tile(coord).await?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Literal(String),
    Quadkey,
    Z,
    X,
    Y,
    /// The row counted from the bottom (TMS), `2^z - 1 - y`.
    FlippedY,
}

/// A pattern that names the file holding a tile, such as `{quadkey}.tif` or `{z}/{x}/{y}.tif`.
///
/// Placeholders are `{quadkey}`, `{z}`, `{x}`, `{y}` and `{-y}` (the row counted from the bottom,
/// as in TMS). They refer to the file's own tile, at the mosaic's file zoom, not to the requested tile.
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyTemplate {
    template: String,
    parts: Vec<Part>,
}

impl KeyTemplate {
    fn new(template: &str) -> Result<Self, MosaicTemplateError> {
        let mut parts = Vec::new();
        let mut rest = template;
        while let Some(start) = rest.find(['{', '}']) {
            let (literal, tail) = rest.split_at(start);
            let Some(tail) = tail.strip_prefix('{') else {
                return Err(MosaicTemplateError::UnmatchedClose);
            };
            if !literal.is_empty() {
                parts.push(Part::Literal(literal.to_owned()));
            }
            let (name, after) = tail
                .split_once('}')
                .ok_or(MosaicTemplateError::UnmatchedOpen)?;
            parts.push(match name {
                "quadkey" => Part::Quadkey,
                "z" => Part::Z,
                "x" => Part::X,
                "y" => Part::Y,
                "-y" => Part::FlippedY,
                _ => return Err(MosaicTemplateError::UnknownPlaceholder(name.to_owned())),
            });
            rest = after;
        }
        if !rest.is_empty() {
            parts.push(Part::Literal(rest.to_owned()));
        }
        let has = |part: &Part| parts.contains(part);
        if !(has(&Part::Quadkey) || has(&Part::X) && (has(&Part::Y) || has(&Part::FlippedY))) {
            return Err(MosaicTemplateError::MissingCoordinates);
        }
        Ok(Self {
            template: template.to_owned(),
            parts,
        })
    }

    /// The name of the file whose own tile is `z/x/y`.
    fn render(&self, z: u8, x: u32, y: u32) -> String {
        let mut key = String::with_capacity(self.template.len() + 16);
        for part in &self.parts {
            match part {
                Part::Literal(text) => key.push_str(text),
                Part::Quadkey => key.push_str(&quadkey(z, x, y)),
                Part::Z => key.push_str(&z.to_string()),
                Part::X => key.push_str(&x.to_string()),
                Part::Y => key.push_str(&y.to_string()),
                Part::FlippedY => key.push_str(&flip_y(z, y).to_string()),
            }
        }
        key
    }

    /// The column and row at `z` of the file named `key`, if `key` is a name this template renders.
    fn parse(&self, key: &str, z: u8) -> Option<(u32, u32)> {
        let (mut x, mut y) = (None, None);
        let mut rest = key;
        for part in &self.parts {
            if let Part::Literal(text) = part {
                rest = rest.strip_prefix(text.as_str())?;
                continue;
            }
            let is_quadkey = matches!(part, Part::Quadkey);
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() && (!is_quadkey || c <= '3')))
                .unwrap_or(rest.len());
            let (value, after) = rest.split_at(end);
            match part {
                Part::Quadkey => {
                    let (qx, qy) = from_quadkey(value, z)?;
                    (x, y) = (Some(qx), Some(qy));
                }
                Part::Z => (value.parse::<u8>().ok()? == z).then_some(())?,
                Part::X => x = Some(value.parse().ok()?),
                Part::Y => y = Some(value.parse().ok()?),
                Part::FlippedY => {
                    let flipped: u32 = value.parse().ok()?;
                    (u64::from(flipped) < 1_u64 << z).then_some(())?;
                    y = Some(flip_y(z, flipped));
                }
                Part::Literal(_) => {}
            }
            rest = after;
        }
        let (x, y) = (x?, y?);
        // Reject names the template would spell differently, e.g. zero-padded numbers.
        (rest.is_empty() && self.render(z, x, y) == key).then_some((x, y))
    }
}

/// Converts a row between XYZ (counted from the top) and TMS (counted from the bottom) at zoom `z`.
fn flip_y(z: u8, y: u32) -> u32 {
    let last = u32::try_from((1_u64 << z) - 1).unwrap_or(u32::MAX);
    last.saturating_sub(y)
}

/// The [Bing Maps quadkey](https://learn.microsoft.com/en-us/bingmaps/articles/bing-maps-tile-system)
/// of tile `z/x/y`: one base-4 digit per zoom level, `0` = NW, `1` = NE, `2` = SW, `3` = SE.
fn quadkey(z: u8, x: u32, y: u32) -> String {
    (1..=z)
        .rev()
        .map(|level| {
            let mask = 1 << (level - 1);
            let digit = u8::from(x & mask != 0) + 2 * u8::from(y & mask != 0);
            char::from(b'0' + digit)
        })
        .collect()
}

/// The column and row of a quadkey, if it has one digit per zoom level up to `z`.
fn from_quadkey(quadkey: &str, z: u8) -> Option<(u32, u32)> {
    if quadkey.len() != usize::from(z) || z > cog_tiles::MAX_ZOOM {
        return None;
    }
    quadkey.bytes().try_fold((0_u32, 0_u32), |(x, y), digit| {
        let digit = match digit {
            b'0'..=b'3' => u32::from(digit - b'0'),
            _ => return None,
        };
        Some(((x << 1) | (digit & 1), (y << 1) | (digit >> 1)))
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path as StdPath;
    use std::sync::Arc;
    use std::time::Duration;

    use cog_tiles::TileFormat;
    use martin_tile_utils::TileCoord;
    use object_store::memory::InMemory;
    use object_store::path::Path;
    use object_store::{ObjectStoreExt as _, PutPayload};
    use rstest::rstest;

    use crate::CacheZoomRange;
    use crate::tiles::cog::{
        CogError, CogMosaicOptions, CogSource, MosaicFileError, MosaicTemplateError,
    };
    use crate::tiles::{MartinCoreError, Source as _};

    const FIXTURES: &str = "../tests/fixtures/cog";
    const MOSAIC_DIR: &str = "../tests/fixtures/cog/mosaic";

    #[rstest]
    #[case("plain.tif", MosaicTemplateError::MissingCoordinates)]
    #[case("{z}/{x}.tif", MosaicTemplateError::MissingCoordinates)]
    #[case("{z}/{-y}.tif", MosaicTemplateError::MissingCoordinates)]
    #[case("{q}.tif", MosaicTemplateError::UnknownPlaceholder("q".to_owned()))]
    #[case("{quadkey.tif", MosaicTemplateError::UnmatchedOpen)]
    #[case("a}{quadkey}", MosaicTemplateError::UnmatchedClose)]
    #[case("../{quadkey}.tif", MosaicTemplateError::NotRelative)]
    #[case("{quadkey}/../x.tif", MosaicTemplateError::NotRelative)]
    #[case("/data/{quadkey}.tif", MosaicTemplateError::NotRelative)]
    #[tokio::test]
    async fn invalid_templates_are_rejected(
        #[case] template: &str,
        #[case] expected: MosaicTemplateError,
    ) {
        let error = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            MOSAIC_DIR.into(),
            template,
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap_err();
        let CogError::InvalidTemplate { reason, .. } = error else {
            panic!("{error:?}");
        };
        assert_eq!(reason, expected);
    }

    #[tokio::test]
    async fn a_file_zoom_above_the_highest_zoom_is_rejected() {
        let error = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            MOSAIC_DIR.into(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 31,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(error, CogError::FileZoomTooHigh { file_zoom: 31 }),
            "{error:?}"
        );
    }

    #[rstest]
    #[case("{quadkey}.tif", "0313131311301120.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
    #[case("sub/{quadkey}.tif", "sub/0313131311301123.tif", "0313131311301123.tif", Some((32_749, 21_795)))]
    #[case("{z}/{x}/{y}.tif", "16/32748/21794.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
    #[case("{x}_{y}.tif", "32748_21794.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
    #[case("L16-{x}E-{-y}N.tif", "L16-32748E-43741N.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
    #[case("{z}/{x}/{-y}.tif", "16/32748/43741.tif", "0313131311301120.tif", Some((32_748, 21_794)))]
    #[case("{x}_{y}.tif", "032748_21794.tif", "0313131311301120.tif", None)]
    #[case("{z}/{x}/{-y}.tif", "16/32748/65536.tif", "0313131311301120.tif", None)]
    #[case("{z}/{x}/{y}.tif", "15/32748/21794.tif", "0313131311301120.tif", None)]
    #[case("{quadkey}.tif", "031313131130112.tif", "0313131311301120.tif", None)]
    #[case("{quadkey}.tif", "0313131311301124.tif", "0313131311301120.tif", None)]
    #[case(
        "{quadkey}.tif",
        "0313131311301120.tif.aux.xml",
        "0313131311301120.tif",
        None
    )]
    #[case("{quadkey}.tif", "readme.txt", "0313131311301120.tif", None)]
    #[case(
        "sub/{quadkey}.tif",
        "0313131311301120.tif",
        "0313131311301120.tif",
        None
    )]
    #[tokio::test]
    async fn file_names_are_matched_to_their_tiles(
        #[case] template: &str,
        #[case] name: &str,
        #[case] fixture: &str,
        #[case] served: Option<(u32, u32)>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::copy(StdPath::new(MOSAIC_DIR).join(fixture), &file).unwrap();
        let result = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            dir.path().to_path_buf(),
            template,
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await;

        let Some((x, y)) = served else {
            let error = result.unwrap_err();
            assert!(matches!(error, CogError::MosaicEmpty { .. }), "{error:?}");
            return;
        };
        let source = result.unwrap();
        let single = CogSource::new(
            "single".to_owned(),
            StdPath::new(MOSAIC_DIR).join(fixture),
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        let xyz = TileCoord::new_unchecked(16, x, y);
        let tile = source.get_tile(xyz, None).await.unwrap();
        assert!(!tile.is_empty());
        assert_eq!(tile, single.get_tile(xyz, None).await.unwrap());
    }

    #[tokio::test]
    async fn a_directory_mosaic_serves_each_tile_from_its_file() {
        let source = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            MOSAIC_DIR.into(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        let tilejson = source.get_tilejson();
        assert_eq!((tilejson.minzoom, tilejson.maxzoom), (Some(16), Some(17)));
        assert_eq!(tilejson.other["tileSize"], 256);
        assert_eq!(tilejson.other["format"], "png");
        assert_eq!(tilejson.bounds, None);

        for (name, x, y) in [
            ("0313131311301120.tif", 32_748, 21_794),
            ("0313131311301121.tif", 32_749, 21_794),
            ("0313131311301122.tif", 32_748, 21_795),
            ("0313131311301123.tif", 32_749, 21_795),
        ] {
            let single = CogSource::new(
                "single".to_owned(),
                StdPath::new(MOSAIC_DIR).join(name),
                CacheZoomRange::default(),
            )
            .await
            .unwrap();
            for xyz in [
                TileCoord::new_unchecked(16, x, y),
                TileCoord::new_unchecked(17, x * 2 + 1, y * 2),
            ] {
                let tile = source.get_tile(xyz, None).await.unwrap();
                assert!(!tile.is_empty(), "{xyz}");
                assert_eq!(tile, single.get_tile(xyz, None).await.unwrap(), "{xyz}");
            }
        }
    }

    #[rstest]
    #[case::above_the_file_zoom(TileCoord::new_unchecked(15, 16_374, 10_897))]
    #[case::no_file_for_this_tile(TileCoord::new_unchecked(16, 32_750, 21_794))]
    #[case::below_a_missing_file(TileCoord::new_unchecked(17, 65_500, 43_588))]
    #[case::deeper_than_the_files_go(TileCoord::new_unchecked(18, 130_992, 87_176))]
    #[case::deeper_than_the_max_zoom(TileCoord::new_unchecked(19, 261_984, 174_352))]
    #[tokio::test]
    async fn tiles_without_a_file_are_empty(#[case] xyz: TileCoord) {
        let source = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            MOSAIC_DIR.into(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: Some(18),
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        assert_eq!(source.get_tilejson().maxzoom, Some(18));
        assert!(source.get_tile(xyz, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_missing_file_stays_missing_until_it_expires() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            StdPath::new(MOSAIC_DIR).join("0313131311301120.tif"),
            dir.path().join("0313131311301120.tif"),
        )
        .unwrap();
        let source = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            dir.path().to_path_buf(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        let ne = TileCoord::new_unchecked(16, 32_749, 21_794);
        assert!(source.get_tile(ne, None).await.unwrap().is_empty());

        std::fs::copy(
            StdPath::new(MOSAIC_DIR).join("0313131311301121.tif"),
            dir.path().join("0313131311301121.tif"),
        )
        .unwrap();
        assert!(source.get_tile(ne, None).await.unwrap().is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn an_opened_file_keeps_serving_after_it_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            StdPath::new(MOSAIC_DIR).join("0313131311301120.tif"),
            dir.path().join("0313131311301120.tif"),
        )
        .unwrap();
        let source = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            dir.path().to_path_buf(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        std::fs::remove_file(dir.path().join("0313131311301120.tif")).unwrap();

        let z17 = TileCoord::new_unchecked(17, 65_497, 43_589);
        assert!(!source.get_tile(z17, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn files_that_do_not_cover_their_tile_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            StdPath::new(MOSAIC_DIR).join("0313131311301120.tif"),
            dir.path().join("0313131311301120.tif"),
        )
        .unwrap();
        let source = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            dir.path().to_path_buf(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        std::fs::copy(
            StdPath::new(MOSAIC_DIR).join("0313131311301120.tif"),
            dir.path().join("0313131311301121.tif"),
        )
        .unwrap();

        let ne = TileCoord::new_unchecked(16, 32_749, 21_794);
        let error = source.get_tile(ne, None).await.unwrap_err();
        let MartinCoreError::CogError(CogError::MosaicFile { key, source }) = error else {
            panic!("{error:?}");
        };
        assert_eq!(key, "0313131311301121.tif");
        let MosaicFileError::DoesNotCover { tile } = *source else {
            panic!("{source:?}");
        };
        assert_eq!((tile.z(), tile.x(), tile.y()), (16, 32_749, 21_794));
    }

    #[tokio::test]
    async fn files_with_a_different_format_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            StdPath::new(MOSAIC_DIR).join("0313131311301120.tif"),
            dir.path().join("0313131311301120.tif"),
        )
        .unwrap();
        let source = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            dir.path().to_path_buf(),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        // A JPEG file covering the whole z15 tile, named as the south-east z16 one.
        std::fs::copy(
            StdPath::new(FIXTURES).join("synthetic/rgb_jpeg.tif"),
            dir.path().join("0313131311301123.tif"),
        )
        .unwrap();

        let se = TileCoord::new_unchecked(16, 32_749, 21_795);
        let error = source.get_tile(se, None).await.unwrap_err();
        let MartinCoreError::CogError(CogError::MosaicFile { key, source }) = error else {
            panic!("{error:?}");
        };
        assert_eq!(key, "0313131311301123.tif");
        assert!(
            matches!(
                *source,
                MosaicFileError::Layout {
                    format: TileFormat::Jpeg,
                    expected_format: TileFormat::Png,
                    ..
                }
            ),
            "{source:?}"
        );
    }

    #[tokio::test]
    async fn a_mosaic_without_matching_files_is_an_error() {
        let error = CogSource::new_mosaic_dir(
            "mosaic".to_owned(),
            MOSAIC_DIR.into(),
            "{quadkey}_missing.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, CogError::MosaicEmpty { .. }), "{error:?}");
    }

    #[tokio::test]
    async fn an_object_store_mosaic_matches_a_directory_mosaic() {
        let store = Arc::new(InMemory::new());
        // Unrelated objects are skipped when looking for a file to probe.
        store
            .put(&Path::from("imagery/readme.txt"), PutPayload::from("hi"))
            .await
            .unwrap();
        for entry in std::fs::read_dir(MOSAIC_DIR).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            let bytes = std::fs::read(&path).unwrap();
            let key = Path::from(format!("imagery/{name}"));
            store.put(&key, PutPayload::from(bytes)).await.unwrap();
        }
        let options = CogMosaicOptions {
            file_zoom: 16,
            max_zoom: None,
            max_cached_files: 100,
            file_expiry: Duration::from_mins(1),
            file_idle_timeout: None,
        };
        let remote = CogSource::new_mosaic_object_store(
            "remote".to_owned(),
            store,
            Path::from("imagery"),
            "{quadkey}.tif",
            "memory://imagery/{quadkey}.tif".to_owned(),
            options,
            CacheZoomRange::default(),
        )
        .await
        .unwrap();
        let local = CogSource::new_mosaic_dir(
            "local".to_owned(),
            MOSAIC_DIR.into(),
            "{quadkey}.tif",
            options,
            CacheZoomRange::default(),
        )
        .await
        .unwrap();

        assert_eq!(remote.get_tilejson(), local.get_tilejson());
        assert_eq!(remote.get_tile_info(), local.get_tile_info());
        let xyz = TileCoord::new_unchecked(17, 65_499, 43_589);
        let tile = remote.get_tile(xyz, None).await.unwrap();
        assert!(!tile.is_empty());
        assert_eq!(tile, local.get_tile(xyz, None).await.unwrap());
        let missing = TileCoord::new_unchecked(16, 32_750, 21_794);
        assert!(remote.get_tile(missing, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_missing_mosaic_directory_is_an_error() {
        let error = CogSource::new_mosaic_dir(
            "missing".to_owned(),
            StdPath::new(MOSAIC_DIR).join("missing"),
            "{quadkey}.tif",
            CogMosaicOptions {
                file_zoom: 16,
                max_zoom: None,
                max_cached_files: 100,
                file_expiry: Duration::from_mins(1),
                file_idle_timeout: None,
            },
            CacheZoomRange::default(),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, CogError::MosaicListing { .. }), "{error:?}");
    }
}
