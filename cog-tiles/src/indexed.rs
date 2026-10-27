//! A mosaic of COGs in any supported projection, found for each tile by an index.

use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use moka::future::Cache;
use object_store::ObjectStore;
use object_store::path::Path;

use crate::warp::{BlockCache, Canvas, WarpedCog};
use crate::{
    CogError, CogResult, DEFAULT_FILE_EXPIRY, DEFAULT_MAX_CACHED_FILES, MosaicFileError, ObjectStoreBackend, TileCoord,
    TileFormat, TilePixels, UnsupportedReason, mercator_to_lnglat,
};

/// The width and height of every tile of an [`AsyncIndexedMosaic`].
pub const INDEXED_TILE_SIZE: u32 = 256;

/// Finds the files that may hold a tile, e.g. by searching a STAC catalog.
#[async_trait::async_trait]
pub trait MosaicIndex: Debug + Send + Sync {
    /// The files that may hold `coord`, in paint order: a pixel is taken from the first file that
    /// has data for it.
    ///
    /// # Errors
    /// Returns an error if the index cannot be searched.
    async fn files_for(&self, coord: TileCoord) -> CogResult<Arc<[MosaicFile]>>;
}

/// One file an [`AsyncIndexedMosaic`] paints tiles from.
#[derive(Clone, Debug)]
pub struct MosaicFile {
    /// Identifies the file in the cache of opened files, so no two files may share it.
    pub cache_key: Arc<str>,
    /// The file's URL or key as shown in errors, without secrets such as signed query strings.
    pub location: Arc<str>,
    /// The store the file is in.
    pub store: Arc<dyn ObjectStore>,
    /// The file's path in `store`.
    pub path: Path,
    /// The file's extent in degrees, `[west, south, east, north]`, so tiles beside it skip it unopened.
    pub bounds: [f64; 4],
}

/// Which tiles an [`AsyncIndexedMosaic`] serves, from which bands, and how many opened files it keeps.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct IndexedMosaicOptions {
    /// The lowest zoom served.
    pub min_zoom: u8,
    /// The highest zoom served. `None` uses the zoom at which the file opened first is fully detailed.
    pub max_zoom: Option<u8>,
    /// The bands of each file to serve, counted from 0: one for gray tiles, three for RGB tiles.
    pub bands: Vec<usize>,
    /// The sample value of pixels without data, overriding each file's `GDAL_NODATA`.
    pub nodata: Option<u8>,
    /// The most opened files kept at once.
    pub max_cached_files: u64,
    /// How long an opened file is kept, so replaced files are picked up eventually.
    pub file_expiry: Duration,
    /// How long an opened file is kept without being used. `None` keeps it until `file_expiry`.
    pub file_idle_timeout: Option<Duration>,
}

impl IndexedMosaicOptions {
    /// Options for tiles from `min_zoom` up made of `bands`, with default cache settings.
    #[must_use]
    pub fn new(min_zoom: u8, bands: Vec<usize>) -> Self {
        Self {
            min_zoom,
            max_zoom: None,
            bands,
            nodata: None,
            max_cached_files: DEFAULT_MAX_CACHED_FILES,
            file_expiry: DEFAULT_FILE_EXPIRY,
            file_idle_timeout: None,
        }
    }

    /// Sets [`max_zoom`](Self::max_zoom).
    #[must_use]
    pub fn with_max_zoom(mut self, max_zoom: Option<u8>) -> Self {
        self.max_zoom = max_zoom;
        self
    }

    /// Sets [`nodata`](Self::nodata).
    #[must_use]
    pub fn with_nodata(mut self, nodata: Option<u8>) -> Self {
        self.nodata = nodata;
        self
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

/// The tiles an [`AsyncIndexedMosaic`] provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexedMosaicInfo {
    /// The lowest zoom with tiles.
    pub min_zoom: u8,
    /// The highest zoom with tiles.
    pub max_zoom: u8,
    /// Tile width and height in pixels.
    pub tile_size: u32,
    /// The format of every tile.
    pub format: TileFormat,
}

/// Reads `WebMercatorQuad` tiles from COGs in any supported projection, which a [`MosaicIndex`]
/// finds for each tile.
///
/// Each tile is painted from the files the index returns, in order: a pixel takes the first file's
/// data for it, so later files only fill what earlier ones leave empty. Painting stops as soon as
/// every pixel is set. Files are reprojected with nearest-neighbour resampling, and every tile is a
/// PNG with an alpha band that is transparent where no file has data.
///
/// Files must be tiled, with 8-bit unsigned samples stored pixel-interleaved, in Web Mercator or a
/// UTM zone. They are opened on first use and kept in a bounded, expiring cache.
#[derive(Debug)]
pub struct AsyncIndexedMosaic {
    /// Finds the files of each tile.
    index: Arc<dyn MosaicIndex>,
    /// Opened files by location.
    readers: Cache<Arc<str>, Arc<WarpedCog<ObjectStoreBackend>>>,
    /// Decoded blocks, shared by every file.
    blocks: BlockCache,
    /// The bands served, counted from 0.
    bands: Vec<usize>,
    /// Overrides each file's nodata value.
    nodata: Option<u8>,
    /// Mosaic metadata.
    info: IndexedMosaicInfo,
}

impl AsyncIndexedMosaic {
    /// Opens a mosaic whose files `index` finds, opening `probe` now to check that it can be read
    /// and to learn the highest zoom if the options leave it unset.
    ///
    /// # Errors
    /// Returns an error if the options select other than one or three bands, or `probe` cannot be
    /// opened, cannot be reprojected, or lacks a selected band.
    pub async fn open(
        index: Arc<dyn MosaicIndex>,
        probe: &MosaicFile,
        options: IndexedMosaicOptions,
    ) -> CogResult<Self> {
        if !matches!(options.bands.len(), 1 | 3) {
            return Err(CogError::MosaicBands {
                count: options.bands.len(),
            });
        }
        let mut readers = Cache::builder()
            .max_capacity(options.max_cached_files)
            .time_to_live(options.file_expiry);
        if let Some(tti) = options.file_idle_timeout {
            readers = readers.time_to_idle(tti);
        }
        let mut mosaic = Self {
            index,
            readers: readers.build(),
            blocks: BlockCache::default(),
            bands: options.bands,
            nodata: options.nodata,
            info: IndexedMosaicInfo {
                min_zoom: options.min_zoom,
                max_zoom: options.min_zoom,
                tile_size: INDEXED_TILE_SIZE,
                format: TileFormat::Png,
            },
        };
        let reader = mosaic.reader(probe).await?;
        let [west, south, east, north] = probe.bounds;
        let center = [f64::midpoint(west, east), f64::midpoint(south, north)];
        let max_zoom = options
            .max_zoom
            .unwrap_or_else(|| reader.native_zoom(center, INDEXED_TILE_SIZE));
        mosaic.info.max_zoom = max_zoom.max(options.min_zoom);
        Ok(mosaic)
    }

    /// What this mosaic offers as a tile source.
    #[must_use]
    pub fn info(&self) -> &IndexedMosaicInfo {
        &self.info
    }

    /// Reads one tile as a PNG.
    ///
    /// Returns `Ok(None)` if the tile is outside the zoom range or no file has data for it.
    ///
    /// # Errors
    /// See [`get_tile_pixels`](Self::get_tile_pixels).
    pub async fn get_tile(&self, coord: TileCoord) -> CogResult<Option<Bytes>> {
        match self.get_tile_pixels(coord).await? {
            Some(pixels) => crate::TileEncoding::Png.encode_at(&pixels, "mosaic").map(Some),
            None => Ok(None),
        }
    }

    /// Reads one tile as pixels of the selected bands plus an alpha band.
    ///
    /// Returns `Ok(None)` like [`get_tile`](Self::get_tile).
    ///
    /// # Errors
    /// Returns an error if the index cannot be searched, or [`CogError::MosaicFile`] if a file
    /// that touches the tile cannot be read or lacks a selected band.
    pub async fn get_tile_pixels(&self, coord: TileCoord) -> CogResult<Option<TilePixels>> {
        if coord.z() < self.info.min_zoom || coord.z() > self.info.max_zoom {
            return Ok(None);
        }
        let files = self.index.files_for(coord).await?;
        let [min_x, min_y, max_x, max_y] = coord.mercator_bounds();
        let [west, south] = mercator_to_lnglat(min_x, min_y);
        let [east, north] = mercator_to_lnglat(max_x, max_y);
        let mut canvas = Canvas::new(INDEXED_TILE_SIZE, self.bands.len());
        for file in files.iter() {
            let [f_west, f_south, f_east, f_north] = file.bounds;
            if f_west >= east || f_east <= west || f_south >= north || f_north <= south {
                continue;
            }
            let reader = self.reader(file).await?;
            reader
                .paint(coord, &self.bands, &mut canvas)
                .await
                .map_err(|source| CogError::MosaicFile {
                    key: file.location.to_string(),
                    source: Arc::new(MosaicFileError::Tiles(source)),
                })?;
            if canvas.is_full() {
                break;
            }
        }
        if canvas.is_empty() {
            return Ok(None);
        }
        canvas.into_pixels("mosaic").map(Some)
    }

    /// Opens (or gets from the cache) `file`, checking that it has the selected bands.
    async fn reader(&self, file: &MosaicFile) -> CogResult<Arc<WarpedCog<ObjectStoreBackend>>> {
        let open = async {
            let backend = ObjectStoreBackend::open(Arc::clone(&file.store), file.path.clone()).await?;
            let reader = WarpedCog::open(backend, self.nodata, self.blocks.clone()).await?;
            if let Some(&band) = self.bands.iter().find(|&&band| band >= reader.samples()) {
                return Err(MosaicFileError::Tiles(CogError::unsupported(
                    reader.location(),
                    UnsupportedReason::MissingBand {
                        band: band + 1,
                        samples: reader.samples(),
                    },
                )));
            }
            Ok(Arc::new(reader))
        };
        self.readers
            .try_get_with(Arc::clone(&file.cache_key), open)
            .await
            .map_err(|source| CogError::MosaicFile {
                key: file.location.to_string(),
                source,
            })
    }
}
