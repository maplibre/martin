//! A mosaic of many COGs, each covering one tile at the mosaic's file zoom.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use bytes::Bytes;
use futures::TryStreamExt as _;
use moka::future::Cache;
use object_store::ObjectStore;
use object_store::local::LocalFileSystem;
use object_store::path::Path;

use crate::{
    AsyncCogReader, CogError, CogInfo, CogResult, FileBackend, KeyTemplate, MAX_ZOOM, ObjectStoreBackend, TileCoord,
    TileFormat, TilePixels,
};

/// The default for [`MosaicOptions::max_cached_files`].
pub const DEFAULT_MAX_CACHED_FILES: u64 = 1000;

/// The default for [`MosaicOptions::file_expiry`].
pub const DEFAULT_FILE_EXPIRY: Duration = Duration::from_mins(10);

/// How a mosaic is laid out and how many opened files it keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct MosaicOptions {
    /// The zoom level at which each file covers exactly one tile, also the lowest zoom served.
    pub file_zoom: u8,
    /// The most opened files (and known-missing ones) kept at once.
    pub max_cached_files: u64,
    /// How long an opened file is kept, so replaced or added files are picked up eventually.
    pub file_expiry: Duration,
    /// How long an opened file is kept without being used. `None` keeps it until `file_expiry`.
    pub file_idle_timeout: Option<Duration>,
}

impl MosaicOptions {
    /// Options for files that each cover one tile at `file_zoom`, with default cache settings.
    #[must_use]
    pub fn new(file_zoom: u8) -> Self {
        Self {
            file_zoom,
            max_cached_files: DEFAULT_MAX_CACHED_FILES,
            file_expiry: DEFAULT_FILE_EXPIRY,
            file_idle_timeout: None,
        }
    }

    /// Sets [`max_cached_files`](Self::max_cached_files).
    #[must_use]
    pub fn with_max_cached_files(mut self, max_cached_files: u64) -> Self {
        self.max_cached_files = max_cached_files;
        self
    }

    /// Sets [`file_expiry`](Self::file_expiry).
    #[must_use]
    pub fn with_file_expiry(mut self, file_expiry: Duration) -> Self {
        self.file_expiry = file_expiry;
        self
    }

    /// Sets [`file_idle_timeout`](Self::file_idle_timeout).
    #[must_use]
    pub fn with_file_idle_timeout(mut self, file_idle_timeout: Option<Duration>) -> Self {
        self.file_idle_timeout = file_idle_timeout;
        self
    }
}

/// The tiles a mosaic provides, learned from the file opened when the mosaic was opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MosaicInfo {
    /// The zoom level at which each file covers exactly one tile, also the lowest zoom with tiles.
    pub file_zoom: u8,
    /// The highest zoom of the file opened first. Other files may go deeper or less deep.
    pub max_zoom: u8,
    /// Tile width and height in pixels, the same in every file.
    pub tile_size: u32,
    /// The format of every tile, the same in every file.
    pub format: TileFormat,
}

/// Why one file of a mosaic cannot be read.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum MosaicFileError {
    /// Cannot open or read the file.
    #[error(transparent)]
    Tiles(#[from] CogError),
    /// The file does not cover the tile its name stands for.
    #[error("it does not cover tile {tile}")]
    DoesNotCover {
        /// The tile the file's name stands for.
        tile: TileCoord,
    },
    /// The file has no tiles at the mosaic's file zoom.
    #[error("its highest zoom {max_zoom} is below the file zoom {file_zoom}")]
    BelowFileZoom {
        /// The file's highest zoom.
        max_zoom: u8,
        /// The mosaic's file zoom.
        file_zoom: u8,
    },
    /// The file's tiles differ in size or format from the first file the mosaic opened.
    #[error(
        "it has {tile_size}px {format:?} tiles, but the mosaic has {expected_tile_size}px {expected_format:?} tiles"
    )]
    Layout {
        /// The file's tile size in pixels.
        tile_size: u32,
        /// The file's tile format.
        format: TileFormat,
        /// The mosaic's tile size in pixels.
        expected_tile_size: u32,
        /// The mosaic's tile format.
        expected_format: TileFormat,
    },
}

/// Reads `WebMercatorQuad` tiles from many COGs, each covering exactly one tile at the file zoom.
///
/// A request for `z/x/y` with `z >= file_zoom` goes to the file of its ancestor tile at the file
/// zoom, whose name comes from a [`KeyTemplate`]. Nothing is listed or indexed: files are opened
/// on first use and kept in a bounded, expiring cache, missing ones included.
///
/// ```no_run
/// # async fn run() -> cog_tiles::CogResult<()> {
/// use cog_tiles::{AsyncCogMosaic, MosaicOptions, TileCoord};
///
/// let mosaic = AsyncCogMosaic::open_dir("/data/ortho", "{quadkey}.tif", MosaicOptions::new(16)).await?;
/// let tile = mosaic.get_tile(TileCoord::new(18, 130_992, 87_176)?).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct AsyncCogMosaic {
    template: KeyTemplate,
    file_zoom: u8,
    files: MosaicFiles,
    readers: Cache<String, Option<Arc<MosaicReader>>>,
    /// The tile size and format of the first file opened, which every other file must share.
    layout: OnceLock<(u32, TileFormat)>,
    info: MosaicInfo,
}

impl AsyncCogMosaic {
    /// Opens a mosaic of local COGs under `root`, named by `template` (e.g. `{quadkey}.tif`
    /// or `{z}/{x}/{y}.tif`) relative to it.
    ///
    /// One file under `root` is opened now to learn the tile size and format;
    /// the others are opened on first use.
    ///
    /// # Errors
    /// Returns an error if the template or file zoom is invalid, `root` cannot be listed,
    /// or no file under it matches the template.
    pub async fn open_dir(root: impl Into<PathBuf>, template: &str, options: MosaicOptions) -> CogResult<Self> {
        let root = root.into();
        let location = root.join(template).display().to_string();
        let lister = LocalFileSystem::new_with_prefix(&root).map_err(|source| CogError::MosaicListing {
            location: location.clone(),
            source,
        })?;
        Self::open(
            template,
            options,
            MosaicFiles::Dir(root),
            &lister,
            &Path::default(),
            &location,
        )
        .await
    }

    /// Opens a mosaic of COGs under `prefix` in `store`, named by `template` (e.g. `{quadkey}.tif`
    /// or `{z}/{x}/{y}.tif`) relative to it.
    ///
    /// `location` is the display form of the mosaic (e.g. a URL without credentials), used in errors.
    /// The first object under `prefix` whose key matches `template` is opened now to learn
    /// the tile size and format, which needs permission to list the prefix.
    /// The others are opened on first use.
    ///
    /// # Errors
    /// Returns an error if the template or file zoom is invalid, `prefix` cannot be listed,
    /// or no object under it matches the template.
    pub async fn open_object_store(
        store: Arc<dyn ObjectStore>,
        prefix: Path,
        template: &str,
        location: &str,
        options: MosaicOptions,
    ) -> CogResult<Self> {
        let files = MosaicFiles::ObjectStore {
            store: Arc::clone(&store),
            prefix: prefix.clone(),
        };
        Self::open(template, options, files, &*store, &prefix, location).await
    }

    async fn open(
        template: &str,
        options: MosaicOptions,
        files: MosaicFiles,
        lister: &dyn ObjectStore,
        prefix: &Path,
        location: &str,
    ) -> CogResult<Self> {
        if options.file_zoom > MAX_ZOOM {
            return Err(CogError::FileZoomTooHigh {
                file_zoom: options.file_zoom,
            });
        }
        let template = KeyTemplate::new(template).map_err(|reason| CogError::InvalidTemplate {
            template: template.to_owned(),
            reason,
        })?;
        let mut readers = Cache::builder()
            .max_capacity(options.max_cached_files)
            .time_to_live(options.file_expiry);
        if let Some(tti) = options.file_idle_timeout {
            readers = readers.time_to_idle(tti);
        }
        let mut mosaic = Self {
            template,
            file_zoom: options.file_zoom,
            files,
            readers: readers.build(),
            layout: OnceLock::new(),
            info: MosaicInfo {
                file_zoom: options.file_zoom,
                max_zoom: options.file_zoom,
                tile_size: 0,
                format: TileFormat::Png,
            },
        };
        let probed = mosaic.probe(lister, prefix, location).await?;
        mosaic.info = MosaicInfo {
            file_zoom: options.file_zoom,
            max_zoom: probed.max_zoom,
            tile_size: probed.tile_size,
            format: probed.format,
        };
        Ok(mosaic)
    }

    /// What this mosaic offers as a tile source.
    #[must_use]
    pub fn info(&self) -> &MosaicInfo {
        &self.info
    }

    /// The template that names each file by its tile at the file zoom.
    #[must_use]
    pub fn template(&self) -> &KeyTemplate {
        &self.template
    }

    /// Reads one tile, encoded as [`MosaicInfo::format`].
    ///
    /// Returns `Ok(None)` if the tile is above the file zoom, no file holds it,
    /// or the file has no tile there.
    ///
    /// # Errors
    /// Returns [`CogError::MosaicFile`] if the file holding the tile cannot be read,
    /// does not cover its tile, or differs in layout from the rest of the mosaic.
    pub async fn get_tile(&self, coord: TileCoord) -> CogResult<Option<Bytes>> {
        match self.reader_for(coord).await? {
            Some(reader) => reader.get_tile(coord).await,
            None => Ok(None),
        }
    }

    /// Reads and decodes one tile, like [`AsyncCogReader::get_tile_pixels`].
    ///
    /// Returns `Ok(None)` like [`get_tile`](Self::get_tile).
    ///
    /// # Errors
    /// See [`get_tile`](Self::get_tile).
    pub async fn get_tile_pixels(&self, coord: TileCoord) -> CogResult<Option<TilePixels>> {
        match self.reader_for(coord).await? {
            Some(reader) => reader.get_tile_pixels(coord).await,
            None => Ok(None),
        }
    }

    /// Opens (or gets from the cache) the file that holds `coord`.
    async fn reader_for(&self, coord: TileCoord) -> CogResult<Option<Arc<MosaicReader>>> {
        let Some(shift) = coord.z().checked_sub(self.file_zoom) else {
            return Ok(None);
        };
        let file_tile = TileCoord::new(self.file_zoom, coord.x() >> shift, coord.y() >> shift)?;
        let key = self.template.render(file_tile);
        let open = async {
            let Some(reader) = self.files.open(&key).await? else {
                return Ok(None);
            };
            for reason in reader.skipped_images() {
                tracing::warn!(cog.location = key, %reason, "Skipping a COG image that cannot be served");
            }
            self.check(reader.info(), file_tile)?;
            Ok(Some(Arc::new(reader)))
        };
        self.readers
            .try_get_with(key.clone(), open)
            .await
            .map_err(|source| CogError::MosaicFile { key, source })
    }

    /// Checks that a file covers its key's tile and matches the rest of the mosaic.
    fn check(&self, info: &CogInfo, tile: TileCoord) -> Result<(), MosaicFileError> {
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
    async fn probe(&self, lister: &dyn ObjectStore, prefix: &Path, location: &str) -> CogResult<CogInfo> {
        let listing_error = |source| CogError::MosaicListing {
            location: location.to_owned(),
            source,
        };
        let mut objects = lister.list(Some(prefix));
        while let Some(object) = objects.try_next().await.map_err(listing_error)? {
            let Some(parts) = object.location.prefix_match(prefix) else {
                continue;
            };
            let key = parts.map(|part| part.as_ref().to_owned()).collect::<Vec<_>>().join("/");
            let Some(tile) = self.template.parse(&key, self.file_zoom) else {
                continue;
            };
            if let Some(reader) = self.reader_for(tile).await? {
                return Ok(*reader.info());
            }
        }
        Err(CogError::MosaicEmpty {
            location: location.to_owned(),
            template: self.template.as_str().to_owned(),
        })
    }
}

/// Where a mosaic's files are.
#[derive(Debug)]
enum MosaicFiles {
    /// Under a local directory.
    Dir(PathBuf),
    /// Under a prefix in an object store.
    ObjectStore { store: Arc<dyn ObjectStore>, prefix: Path },
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
fn is_not_found(error: &CogError) -> bool {
    let CogError::Backend { source, .. } = error else {
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

    fn skipped_images(&self) -> &[crate::SkippedImage] {
        match self {
            Self::File(reader) => reader.skipped_images(),
            Self::ObjectStore(reader) => reader.skipped_images(),
        }
    }

    async fn get_tile(&self, coord: TileCoord) -> CogResult<Option<Bytes>> {
        match self {
            Self::File(reader) => reader.get_tile(coord).await,
            Self::ObjectStore(reader) => reader.get_tile(coord).await,
        }
    }

    async fn get_tile_pixels(&self, coord: TileCoord) -> CogResult<Option<TilePixels>> {
        match self {
            Self::File(reader) => reader.get_tile_pixels(coord).await,
            Self::ObjectStore(reader) => reader.get_tile_pixels(coord).await,
        }
    }
}
