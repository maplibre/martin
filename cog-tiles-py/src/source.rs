//! One COG or a mosaic of them, and the concurrent reads the Python API is built from.

use std::sync::Arc;

use bytes::Bytes;
use cog_tiles::{
    AsyncCogMosaic, AsyncCogReader, CogResult, FileBackend, MosaicOptions, ObjectStoreBackend, PixelWindow, TileCoord,
    TileFormat, TilePixels,
};
use futures::{StreamExt as _, stream};
use pyo3::PyResult;
use tokio::sync::mpsc;
use tokio_util::task::AbortOnDropHandle;

use crate::error::{CogTilesError, to_py};
use crate::location::Location;

/// Where tiles are read from.
pub enum Source {
    File(AsyncCogReader<FileBackend>),
    Store(AsyncCogReader<ObjectStoreBackend>),
    Mosaic(AsyncCogMosaic),
}

/// The zooms, tile size and format of a source.
#[derive(Clone, Copy)]
pub struct Layout {
    pub min_zoom: u8,
    pub max_zoom: u8,
    pub tile_size: u32,
    pub format: TileFormat,
}

impl Source {
    pub async fn open(location: Location) -> CogResult<Self> {
        Ok(match location {
            Location::Local(path) => {
                Self::File(AsyncCogReader::try_from_backend(FileBackend::open(path).await?).await?)
            }
            Location::Store { store, path, .. } => {
                Self::Store(AsyncCogReader::try_from_backend(ObjectStoreBackend::open(store, path).await?).await?)
            }
        })
    }

    pub async fn open_mosaic(location: Location, template: &str, options: MosaicOptions) -> CogResult<Self> {
        Ok(Self::Mosaic(match location {
            Location::Local(root) => AsyncCogMosaic::open_dir(root, template, options).await?,
            Location::Store { store, path, display } => {
                AsyncCogMosaic::open_object_store(store, path, template, &format!("{display}{template}"), options)
                    .await?
            }
        }))
    }

    pub fn layout(&self) -> Layout {
        match self {
            Self::File(reader) => reader_layout(reader),
            Self::Store(reader) => reader_layout(reader),
            Self::Mosaic(mosaic) => {
                let info = mosaic.info();
                Layout {
                    min_zoom: info.file_zoom,
                    max_zoom: info.max_zoom,
                    tile_size: info.tile_size,
                    format: info.format,
                }
            }
        }
    }

    /// `[west, south, east, north]` in degrees and `[min_x, min_y, max_x, max_y]` in `EPSG:3857`,
    /// or `None` for a mosaic, whose extent is not known without listing every file.
    pub fn bounds(&self) -> Option<([f64; 4], [f64; 4])> {
        let info = match self {
            Self::File(reader) => reader.info(),
            Self::Store(reader) => reader.info(),
            Self::Mosaic(_) => return None,
        };
        Some((info.bounds, info.mercator_bounds))
    }

    pub async fn tile(&self, coord: TileCoord) -> CogResult<Option<Bytes>> {
        match self {
            Self::File(reader) => reader.get_tile(coord).await,
            Self::Store(reader) => reader.get_tile(coord).await,
            Self::Mosaic(mosaic) => mosaic.get_tile(coord).await,
        }
    }

    pub async fn pixels(&self, coord: TileCoord) -> CogResult<Option<TilePixels>> {
        match self {
            Self::File(reader) => reader.get_tile_pixels(coord).await,
            Self::Store(reader) => reader.get_tile_pixels(coord).await,
            Self::Mosaic(mosaic) => mosaic.get_tile_pixels(coord).await,
        }
    }
}

fn reader_layout<B: cog_tiles::AsyncBackend>(reader: &AsyncCogReader<B>) -> Layout {
    let info = reader.info();
    Layout {
        min_zoom: info.min_zoom,
        max_zoom: info.max_zoom,
        tile_size: info.tile_size,
        format: info.format,
    }
}

/// Reads and decodes every tile `window` touches, `concurrency` at a time, each on its own task
/// so decoding uses every worker thread, and pastes them into one raster.
///
/// Returns the raster and its band count, or `None` if no tile has data.
pub async fn read_window(
    source: Arc<Source>,
    window: PixelWindow,
    concurrency: usize,
) -> PyResult<Option<(Vec<u8>, usize)>> {
    let mut decoded = stream::iter(window.tiles())
        .map(|coord| {
            let source = Arc::clone(&source);
            AbortOnDropHandle::new(tokio::spawn(async move { (coord, source.pixels(coord).await) }))
        })
        .buffer_unordered(concurrency);
    let pixel_count = window.width() as usize * window.height() as usize;
    let mut raster: Option<(Vec<u8>, usize)> = None;
    while let Some(joined) = decoded.next().await {
        let (coord, pixels) = joined.map_err(|e| CogTilesError::new_err(e.to_string()))?;
        let Some(pixels) = pixels.map_err(to_py)? else {
            continue;
        };
        let (data, bands) = raster.get_or_insert_with(|| (vec![0; pixel_count * pixels.bands()], pixels.bands()));
        if *bands != pixels.bands() {
            return Err(CogTilesError::new_err(format!(
                "tile {coord} has {} bands, but earlier tiles have {bands}",
                pixels.bands()
            )));
        }
        window.paste(data, coord, &pixels);
    }
    Ok(raster)
}

/// Sends every tile with data that `windows` touch to `sender`, `concurrency` at a time, in the
/// order they arrive. Stops at the first error, which is sent too, or when the receiver is dropped.
pub async fn stream_tiles(
    source: Arc<Source>,
    windows: Vec<PixelWindow>,
    concurrency: usize,
    sender: mpsc::Sender<PyResult<(TileCoord, Bytes)>>,
) {
    let mut tiles = stream::iter(windows.into_iter().flat_map(|window| window.tiles()))
        .map(|coord| {
            let source = Arc::clone(&source);
            AbortOnDropHandle::new(tokio::spawn(async move { (coord, source.tile(coord).await) }))
        })
        .buffer_unordered(concurrency);
    while let Some(joined) = tiles.next().await {
        let item = match joined {
            Ok((_, Ok(None))) => continue,
            Ok((coord, Ok(Some(data)))) => Ok((coord, data)),
            Ok((_, Err(e))) => Err(to_py(e)),
            Err(e) => Err(CogTilesError::new_err(e.to_string())),
        };
        let failed = item.is_err();
        if sender.send(item).await.is_err() || failed {
            return;
        }
    }
}
