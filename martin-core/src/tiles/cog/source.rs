use std::fmt::Debug;
use std::path::PathBuf;
use std::sync::Arc;

use cog_tiles::{
    AsyncBackend as _, AsyncCogMosaic, AsyncCogReader, AsyncIndexedMosaic, CogInfo, FileBackend, ObjectStoreBackend,
    TileEncoding, TileFormat, TilePixels,
};
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
use object_store::ObjectStore;
use serde_json::Value;
use tilejson::{Bounds, Center, TileJSON, tilejson};
use tracing::instrument;

use crate::CacheZoomRange;
use crate::tiles::cog::CogError;
use crate::tiles::source::cap_zoom_range;
use crate::tiles::{MartinCoreResult, Source, UrlQuery};

/// Where the tiles of a [`CogSource`] come from.
#[derive(Clone, Debug)]
pub(super) enum CogTiles {
    File(Arc<AsyncCogReader<FileBackend>>),
    ObjectStore(Arc<AsyncCogReader<ObjectStoreBackend>>),
    Mosaic(Arc<AsyncCogMosaic>),
    Indexed(Arc<AsyncIndexedMosaic>),
}

impl CogTiles {
    /// Reads one tile as stored, or as PNG where it cannot be passed through, unless `encoding` re-encodes it.
    async fn read(
        &self,
        coord: cog_tiles::TileCoord,
        encoding: Option<TileEncoding>,
        id: &str,
    ) -> Result<Option<TileData>, CogError> {
        match self {
            Self::File(reader) => {
                read_as(
                    || reader.get_tile(coord),
                    || reader.get_tile_pixels(coord),
                    encoding,
                    id,
                )
                .await
            }
            Self::ObjectStore(reader) => {
                read_as(
                    || reader.get_tile(coord),
                    || reader.get_tile_pixels(coord),
                    encoding,
                    id,
                )
                .await
            }
            Self::Mosaic(mosaic) => {
                read_as(
                    || mosaic.get_tile(coord),
                    || mosaic.get_tile_pixels(coord),
                    encoding,
                    id,
                )
                .await
            }
            Self::Indexed(mosaic) => {
                read_as(
                    || mosaic.get_tile(coord),
                    || mosaic.get_tile_pixels(coord),
                    encoding,
                    id,
                )
                .await
            }
        }
    }
}

/// Reads `stored` without an `encoding`, else reads `pixels` and encodes them on the blocking thread pool.
async fn read_as<S, P>(
    stored: impl FnOnce() -> S,
    pixels: impl FnOnce() -> P,
    encoding: Option<TileEncoding>,
    id: &str,
) -> Result<Option<TileData>, CogError>
where
    S: Future<Output = cog_tiles::CogResult<Option<TileData>>>,
    P: Future<Output = cog_tiles::CogResult<Option<TilePixels>>>,
{
    let Some(encoding) = encoding else {
        return Ok(stored().await?);
    };
    let Some(pixels) = pixels().await? else {
        return Ok(None);
    };
    let id = id.to_owned();
    let tile = tokio::task::spawn_blocking(move || encoding.encode(&pixels, &id))
        .await
        .map_err(|join| CogError::Encode { join })??;
    Ok(Some(tile))
}

/// Tile source that reads from one `Cloud Optimized GeoTIFF` file, or from a mosaic of them.
///
/// Every COG must be tiled on the `WebMercatorQuad` grid, so each served tile is one internal
/// tile of a file: JPEG and WebP tiles are returned as stored, others are re-encoded as PNG,
/// unless [`with_encoding`](Self::with_encoding) re-encodes every tile.
#[derive(Clone, Debug)]
pub struct CogSource {
    id: String,
    tiles: CogTiles,
    encoding: Option<TileEncoding>,
    min_zoom: u8,
    max_zoom: u8,
    tilejson: TileJSON,
    tileinfo: TileInfo,
    cache_zoom: CacheZoomRange,
}

impl CogSource {
    /// Creates a new COG tile source from a file path.
    pub async fn new(id: String, path: PathBuf, cache_zoom: CacheZoomRange) -> Result<Self, CogError> {
        let reader = AsyncCogReader::try_from_backend(FileBackend::open(path).await?).await?;
        let location = reader.backend().location().to_owned();
        warn_skipped_images(&location, reader.skipped_images());
        let info = *reader.info();
        Self::from_info(id, CogTiles::File(Arc::new(reader)), &info, location, cache_zoom)
    }

    /// Creates a COG source backed by an arbitrary `object_store` implementation.
    ///
    /// `location` is the display form of the object (e.g. a URL without credentials), used in errors.
    pub async fn new_object_store(
        id: String,
        store: Arc<dyn ObjectStore>,
        object_path: object_store::path::Path,
        location: String,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let backend = ObjectStoreBackend::open(store, object_path).await?;
        let reader = AsyncCogReader::try_from_backend(backend).await?;
        warn_skipped_images(&location, reader.skipped_images());
        let info = *reader.info();
        Self::from_info(id, CogTiles::ObjectStore(Arc::new(reader)), &info, location, cache_zoom)
    }

    fn from_info(
        id: String,
        tiles: CogTiles,
        info: &CogInfo,
        location: String,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let [west, south, east, north] = info.bounds;
        let tilejson = tilejson! {
            tiles: vec![],
            bounds: Bounds::new(west, south, east, north),
            center: Center {
                longitude: f64::midpoint(west, east),
                latitude: center_latitude(info),
                zoom: u8::midpoint(info.max_zoom, info.min_zoom),
            },
            minzoom: info.min_zoom,
            maxzoom: info.max_zoom,
        };
        Self::with_layout(id, tiles, tilejson, (info.tile_size, info.format), location, cache_zoom)
    }

    /// Finishes a source whose `TileJSON` has its zoom range set, adding the tile layout to it.
    pub(super) fn with_layout(
        id: String,
        tiles: CogTiles,
        mut tilejson: TileJSON,
        (tile_size, tile_format): (u32, TileFormat),
        location: String,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        cap_zoom_range(&id, &mut tilejson);
        let format = martin_format(tile_format).ok_or(CogError::UnsupportedFormat {
            location,
            format: tile_format,
        })?;
        tilejson.other.insert("tileSize".to_owned(), Value::from(tile_size));
        let mut source = Self {
            id,
            tiles,
            encoding: None,
            min_zoom: tilejson.minzoom.unwrap_or_default(),
            max_zoom: tilejson.maxzoom.unwrap_or_default(),
            tilejson,
            tileinfo: TileInfo::new(format, Encoding::Internal),
            cache_zoom,
        };
        source.set_format(format);
        Ok(source)
    }

    /// Decodes every tile and encodes it as `encoding` if set, even tiles stored as JPEG or WebP.
    #[must_use]
    pub fn with_encoding(mut self, encoding: Option<TileEncoding>) -> Self {
        if let Some(encoding) = encoding {
            let format = martin_format(encoding.format()).expect("every encoding is a format Martin serves");
            self.encoding = Some(encoding);
            self.set_format(format);
        }
        self
    }

    /// Reports `format` as the format of every tile.
    fn set_format(&mut self, format: Format) {
        self.tileinfo = TileInfo::new(format, Encoding::Internal);
        self.tilejson
            .other
            .insert("format".to_owned(), Value::from(format.to_string()));
    }
}

/// The Martin form of a tile format, if Martin serves it.
fn martin_format(format: TileFormat) -> Option<Format> {
    match format {
        TileFormat::Jpeg => Some(Format::Jpeg),
        TileFormat::Webp => Some(Format::Webp),
        TileFormat::Png => Some(Format::Png),
        _ => None,
    }
}

/// Logs the images of a COG that are not served, e.g. overviews that are off the tile grid.
fn warn_skipped_images(location: &str, skipped: &[cog_tiles::SkippedImage]) {
    for reason in skipped {
        tracing::warn!(
            cog.location = location,
            %reason,
            "Skipping a COG image that cannot be served"
        );
    }
}

/// The `cog-tiles` form of a tile coordinate.
fn cog_coord(xyz: TileCoord) -> Result<cog_tiles::TileCoord, CogError> {
    Ok(cog_tiles::TileCoord::new(xyz.z(), xyz.x(), xyz.y())?)
}

/// The latitude halfway between the image's top and bottom in Web Mercator.
fn center_latitude(info: &CogInfo) -> f64 {
    let [_, min_y, _, max_y] = info.mercator_bounds;
    cog_tiles::mercator_to_lnglat(0.0, f64::midpoint(min_y, max_y))[1]
}

impl Source for CogSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tileinfo
    }

    /// Whether this [`Source`] benefits from concurrency when being scraped via `martin cp`.
    ///
    /// If this returns `true`, `martin cp` will suggest concurrent scraping.
    fn benefits_from_concurrent_scraping(&self) -> bool {
        true
    }

    fn cache_zoom(&self) -> CacheZoomRange {
        self.cache_zoom
    }

    #[instrument(
        level = "debug",
        skip_all,
        fields(
            source.id = %self.id,
            tile.z = xyz.z(),
            tile.x = xyz.x(),
            tile.y = xyz.y(),
        ),
        err(Debug),
    )]
    async fn get_tile(&self, xyz: TileCoord, _url_query: Option<&UrlQuery>) -> MartinCoreResult<TileData> {
        if xyz.z() < self.min_zoom || xyz.z() > self.max_zoom {
            return Ok(TileData::default());
        }
        let tile = self.tiles.read(cog_coord(xyz)?, self.encoding, &self.id).await?;
        Ok(tile.unwrap_or_default())
    }
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::ffi::OsStr;
    #[cfg(target_os = "linux")]
    use std::os::unix::ffi::OsStrExt as _;
    use std::path::Path;
    use std::sync::Arc;

    use approx::{abs_diff_eq, assert_abs_diff_eq};
    use martin_tile_utils::TileCoord;
    use object_store::memory::InMemory;
    use object_store::{ObjectStoreExt as _, PutPayload};
    use rstest::rstest;
    use tilejson::{Bounds, Center};

    use crate::CacheZoomRange;
    use crate::tiles::Source as _;
    use crate::tiles::cog::{CogError, CogSource};

    #[tokio::test]
    async fn malformed_metadata_is_an_error_instead_of_a_panic() {
        fn entry(bytes: &mut Vec<u8>, tag: u16, field_type: u16, value: u32) {
            bytes.extend_from_slice(&tag.to_le_bytes());
            bytes.extend_from_slice(&field_type.to_le_bytes());
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            bytes.extend_from_slice(&value.to_le_bytes());
        }

        // A syntactically valid little-endian TIFF IFD that deliberately omits
        // SamplesPerPixel. async-tiff 0.3 panics while constructing this IFD.
        let mut bytes = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        entry(&mut bytes, 256, 4, 128); // ImageWidth
        entry(&mut bytes, 257, 4, 128); // ImageLength
        entry(&mut bytes, 258, 3, 8); // BitsPerSample
        entry(&mut bytes, 262, 3, 2); // PhotometricInterpretation
        bytes.extend_from_slice(&0_u32.to_le_bytes());

        let store = Arc::new(InMemory::new());
        let path = object_store::path::Path::from("malformed.tif");
        store.put(&path, PutPayload::from(bytes)).await.unwrap();
        let result = CogSource::new_object_store(
            "malformed".to_owned(),
            store,
            path,
            "memory://malformed.tif".to_owned(),
            CacheZoomRange::default(),
        )
        .await;

        assert!(
            matches!(result, Err(CogError::Tiles(cog_tiles::CogError::Unsupported { .. }))),
            "{result:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn opens_a_local_cog_with_a_non_utf8_filename() {
        let temp = tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target")).unwrap();
        let path = temp.path().join(OsStr::from_bytes(b"image-\xff.tif"));
        std::fs::copy("../tests/fixtures/cog/usda_naip_128_none_z2.tif", &path).unwrap();

        let source = CogSource::new("non-utf8".to_owned(), path, CacheZoomRange::default())
            .await
            .unwrap();
        let tile = source
            .get_tile(TileCoord::new_unchecked(18, 42_712, 97_343), None)
            .await
            .unwrap();

        assert!(!tile.is_empty());
    }

    #[tokio::test]
    async fn opens_a_cog_with_more_than_32_kib_of_metadata() {
        let source = CogSource::new(
            "large-metadata".to_owned(),
            Path::new("../tests/fixtures/cog/regressions/usda_naip_128_large_metadata.tif").to_path_buf(),
            CacheZoomRange::default(),
        )
        .await
        .unwrap();

        assert_eq!(source.min_zoom, 18);
        assert_eq!(source.max_zoom, 19);
    }

    #[tokio::test]
    async fn sparse_tile_is_returned_as_empty() {
        let source = CogSource::new(
            "sparse".to_owned(),
            Path::new("../tests/fixtures/cog/regressions/usda_naip_128_none_sparse.tif").to_path_buf(),
            CacheZoomRange::default(),
        )
        .await
        .unwrap();

        let tile = source
            .get_tile(TileCoord::new_unchecked(19, 85_424, 194_686), None)
            .await
            .unwrap();
        assert!(tile.is_empty());
    }

    #[tokio::test]
    async fn tiles_outside_the_zoom_range_or_the_image_are_empty() {
        let source = CogSource::new(
            "outside".to_owned(),
            Path::new("../tests/fixtures/cog/usda_naip_128_none_z2.tif").to_path_buf(),
            CacheZoomRange::default(),
        )
        .await
        .unwrap();

        for xyz in [
            TileCoord::new_unchecked(17, 21_356, 48_671),
            TileCoord::new_unchecked(20, 170_848, 389_372),
            TileCoord::new_unchecked(18, 0, 0),
        ] {
            let tile = source.get_tile(xyz, None).await.unwrap();
            assert!(tile.is_empty(), "{xyz} should be empty");
        }
    }

    #[rstest]
    #[case("usda_naip_256_lzw_z3".to_owned(), Center {
        longitude: -121.346_740_722_656_22,
        latitude: 41.967_659_203_678_16,
        zoom: 17,
    }, Bounds {
        left: -121.349_487_304_687_46,
        top: 41.971_743_363_279_65,
        right: -121.343_994_140_624_97,
        bottom: 41.963_574_782_225_15,
    }, 16, 18, 256, "png")]
    #[case("usda_naip_512_deflate_z2".to_owned(), Center {
        longitude: -121.346_740_722_656_22,
        latitude: 41.967_659_203_678_16,
        zoom: 16,
    }, Bounds {
        left: -121.349_487_304_687_46,
        top: 41.971_743_363_279_65,
        right: -121.343_994_140_624_97,
        bottom: 41.963_574_782_225_15,
    }, 16, 17, 512, "png")]
    #[case("usda_naip_512_jpeg_z5".to_owned(), Center {
        longitude: -121.354_980_468_749_96,
        latitude: 41.967_659_203_678_146,
        zoom: 15,
    }, Bounds {
        left: -121.376_953_124_999_94,
        top: 42.000_325_148_316_2,
        right: -121.333_007_812_499_96,
        bottom: 41.934_976_500_546_576,
    }, 13, 17, 512, "jpeg")]
    #[case("usda_naip_512_webp_z5".to_owned(), Center {
        longitude: -121.354_980_468_749_96,
        latitude: 41.967_659_203_678_146,
        zoom: 15,
    }, Bounds {
        left: -121.376_953_124_999_94,
        top: 42.000_325_148_316_2,
        right: -121.333_007_812_499_96,
        bottom: 41.934_976_500_546_576,
    }, 13, 17, 512, "webp")]
    #[case("usda_naip_128_none_z2".to_owned(), Center {
        longitude: -121.343_650_817_871_1,
        latitude: 41.968_169_737_948_436,
        zoom: 18,
    }, Bounds {
        left: -121.343_994_140_625,
        top: 41.968_680_268_127_265,
        right: -121.343_307_495_117_19,
        bottom: 41.967_659_203_678_17,
    }, 18, 19, 128, "png")]
    #[tokio::test]
    async fn can_generate_tilejson_from_source(
        #[case] cog_file: String,
        #[case] center: Center,
        #[case] bounds: Bounds,
        #[case] min_zoom: u8,
        #[case] max_zoom: u8,
        #[case] tile_size: u32,
        #[case] format: String,
    ) {
        let path = format!("../tests/fixtures/cog/{cog_file}.tif");
        let source = CogSource::new(cog_file, Path::new(&path).to_path_buf(), CacheZoomRange::default())
            .await
            .unwrap();

        assert_eq!(source.max_zoom, max_zoom);
        assert_eq!(source.min_zoom, min_zoom);
        let actual_center = source.tilejson.center.unwrap();
        assert!(abs_diff_eq!(actual_center.longitude, center.longitude, epsilon = 1e-12));
        assert!(abs_diff_eq!(actual_center.latitude, center.latitude, epsilon = 1e-12));
        assert_eq!(actual_center.zoom, center.zoom);
        let actual_bounds = source.tilejson.bounds.unwrap();
        assert_abs_diff_eq!(actual_bounds.left, bounds.left, epsilon = 1e-12);
        assert_abs_diff_eq!(actual_bounds.bottom, bounds.bottom, epsilon = 1e-12);
        assert_abs_diff_eq!(actual_bounds.right, bounds.right, epsilon = 1e-12);
        assert_abs_diff_eq!(actual_bounds.top, bounds.top, epsilon = 1e-12);
        assert_eq!(source.tilejson.other.get("tileSize").unwrap(), tile_size);
        assert_eq!(
            source.tilejson.other.get("format").unwrap().as_str(),
            Some(format.as_str())
        );
    }
}
