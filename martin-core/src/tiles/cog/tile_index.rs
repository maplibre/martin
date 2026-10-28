//! A COG source over a tile index: a `GeoPackage` with one footprint and location per file, as written by `gdaltindex`.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use cog_tiles::{AsyncIndexedMosaic, HrefResolver, IndexedMosaicOptions, TileIndex};
use tilejson::{Bounds, Center, tilejson};

use crate::CacheZoomRange;
use crate::tiles::cog::CogError;
use crate::tiles::cog::source::{CogSource, CogTiles};

/// A source over the files of a tile index: where the index is, and how to paint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CogTileIndexOptions {
    /// The `GeoPackage` file.
    pub path: PathBuf,
    /// The feature table to read. `None` reads the only feature table of the file.
    pub layer: Option<String>,
    /// The column holding each file's location.
    pub location_field: String,
    /// The lowest zoom served.
    pub min_zoom: u8,
    /// The highest zoom served. `None` uses the zoom at which the first file is fully detailed.
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

impl CogSource {
    /// Creates a source over the files of a tile index, resolving their locations with `resolve`.
    ///
    /// The whole index is read now, and its first file is opened to check that it can be served.
    ///
    /// # Errors
    /// Returns an error if the index cannot be read (see [`TileIndex::open`]) or its first file cannot
    /// be served.
    pub async fn new_tile_index(
        id: String,
        options: CogTileIndexOptions,
        resolve: &HrefResolver,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let index = TileIndex::open(
            &options.path,
            options.layer.as_deref(),
            &options.location_field,
            resolve,
        )
        .await?;
        let [west, south, east, north] = index.bounds();
        let mosaic_options = IndexedMosaicOptions::new(options.min_zoom, options.bands)
            .with_max_zoom(options.max_zoom)
            .with_nodata(options.nodata)
            .with_max_cached_files(options.max_cached_files)
            .with_file_expiry(options.file_expiry)
            .with_file_idle_timeout(options.file_idle_timeout);
        let mosaic = AsyncIndexedMosaic::open_tile_index(index, mosaic_options).await?;
        let info = *mosaic.info();
        let tilejson = tilejson! {
            tiles: vec![],
            bounds: Bounds::new(west, south, east, north),
            center: Center {
                longitude: f64::midpoint(west, east),
                latitude: f64::midpoint(south, north),
                zoom: info.min_zoom,
            },
            minzoom: info.min_zoom,
            maxzoom: info.max_zoom,
        };
        Self::with_layout(
            id,
            CogTiles::Indexed(Arc::new(mosaic)),
            tilejson,
            (info.tile_size, info.format),
            options.path.display().to_string(),
            cache_zoom,
        )
    }
}
