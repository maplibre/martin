use std::collections::HashMap;
use std::fmt::Debug;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::vec;

use async_tiff::ImageFileDirectory;
use async_tiff::metadata::TiffMetadataReader;
use async_tiff::metadata::cache::ReadaheadMetadataCache;
use async_tiff::tags::{Compression, PhotometricInterpretation, PlanarConfiguration};
use async_trait::async_trait;
use futures::FutureExt as _;
use martin_tile_utils::{
    EARTH_CIRCUMFERENCE, Encoding, MAX_ZOOM, TileCoord, TileData, TileInfo, webmercator_to_wgs84,
};
use object_store::ObjectStore;
use serde_json::Value;
use tilejson::{Bounds, Center, TileJSON, tilejson};
use tracing::instrument;

use crate::CacheZoomRange;
use crate::tiles::cog::CogError;
use crate::tiles::cog::image::Image;
use crate::tiles::cog::model::ModelInfo;
use crate::tiles::cog::reader::{AsyncTiffMetadataReader, LocalFileCogReader};
use crate::tiles::cog::{CogReader, ObjectStoreCogReader};
use crate::tiles::{MartinCoreResult, Source, UrlQuery};

/// Maximum allowed relative error (as a fraction) when matching a resolution to a `WebMercatorQuad`
/// tile matrix zoom level. 1e-3 = 0.1%.
pub const MAX_RESOLUTION_ERROR: f64 = 1e-3;

/// Maximum allowed absolute error (in meters) when matching a resolution to a `WebMercatorQuad`
/// tile matrix zoom level. Caps the relative threshold at low zoom levels where 0.1% would
/// otherwise permit hundreds of meters of error.
pub const MAX_ABSOLUTE_RESOLUTION_ERROR: f64 = 3.0;

/// Tile source that reads from `Cloud Optimized GeoTIFF` files.
#[derive(Clone, Debug)]
pub struct CogSource {
    id: String,
    location: String,
    reader: Arc<dyn CogReader>,
    min_zoom: u8,
    max_zoom: u8,
    images: HashMap<u8, Image>,
    tilejson: TileJSON,
    tileinfo: TileInfo,
    cache_zoom: CacheZoomRange,
}

impl CogSource {
    /// Creates a new COG tile source from a file path.
    pub async fn new(
        id: String,
        path: PathBuf,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let reader: Arc<dyn CogReader> = Arc::new(LocalFileCogReader::try_new(path).await?);
        Self::new_reader(id, reader, cache_zoom).await
    }

    /// Creates a COG source backed by an arbitrary `object_store` implementation.
    pub async fn new_object_store(
        id: String,
        store: Arc<dyn ObjectStore>,
        object_path: object_store::path::Path,
        location: String,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let reader: Arc<dyn CogReader> =
            Arc::new(ObjectStoreCogReader::try_new(store, object_path, location).await?);
        Self::new_reader(id, reader, cache_zoom).await
    }

    #[expect(clippy::too_many_lines)]
    async fn new_reader(
        id: String,
        reader: Arc<dyn CogReader>,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, CogError> {
        let location = reader.location().to_owned();
        let adapter = AsyncTiffMetadataReader(Arc::clone(&reader));
        let initial_size = reader.metadata().size.min(32 * 1024);
        let cached_reader = ReadaheadMetadataCache::new(adapter).with_initial_size(initial_size);
        let parsed = AssertUnwindSafe(async {
            let mut metadata_reader = TiffMetadataReader::try_open(&cached_reader).await?;
            metadata_reader.read_all_ifds(&cached_reader).await
        })
        .catch_unwind()
        .await;
        let ifds = match parsed {
            Ok(Ok(ifds)) => ifds,
            Ok(Err(error)) => return Err(CogError::AsyncTiff(error, location.clone())),
            Err(payload) => {
                let reason = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("unknown parser panic");
                return Err(CogError::AsyncTiff(
                    async_tiff::error::AsyncTiffError::General(format!(
                        "TIFF metadata parser panicked: {reason}"
                    )),
                    location.clone(),
                ));
            }
        };
        let base_ifd = ifds
            .first()
            .ok_or_else(|| CogError::NoImagesFound(PathBuf::from(&location)))?;
        let model = ModelInfo::decode(base_ifd);
        verify_requirements(base_ifd, &model, Path::new(&location))?;
        let origin = get_origin(
            model.tie_points.as_deref(),
            model.transformation.as_deref(),
            Path::new(&location),
        )?;
        let (full_width_pixel, full_length_pixel) =
            (base_ifd.image_width(), base_ifd.image_height());
        let (full_width, full_length) = dimensions_in_model(
            base_ifd,
            Path::new(&location),
            model.pixel_scale.as_deref(),
            model.transformation.as_deref(),
        )?;
        let extent = get_extent(
            &origin,
            model.transformation.as_deref(),
            (full_width_pixel, full_length_pixel),
            (full_width, full_length),
        );

        let mut images = vec![];
        for ifd in ifds {
            let is_source_image = ifd.new_subfile_type().is_none();
            let is_reduced_resolution_subfile = ifd.new_subfile_type() == Some(0b001);
            if is_source_image || is_reduced_resolution_subfile {
                let image_width = ifd.image_width();
                let resolution = full_width / f64::from(image_width);
                images.push(get_image(
                    Arc::new(ifd),
                    Path::new(&location),
                    origin,
                    resolution,
                )?);
            }
        }

        let images: HashMap<u8, Image> = images
            .into_iter()
            .map(|image| (image.zoom_level(), image))
            .collect();

        let mut tile_size = None;
        for image in images.values() {
            match tile_size {
                Some(current_tile_size) => {
                    if current_tile_size != image.tile_size() {
                        return Err(CogError::InconsistentTiling(PathBuf::from(&location)));
                    }
                }
                None => {
                    tile_size = Some(image.tile_size());
                }
            }
        }
        let min_zoom = *images
            .keys()
            .min()
            .ok_or_else(|| CogError::NoImagesFound(PathBuf::from(&location)))?;
        let max_zoom = *images
            .keys()
            .max()
            .ok_or_else(|| CogError::NoImagesFound(PathBuf::from(&location)))?;
        let min = webmercator_to_wgs84(extent[0], extent[1]);
        let max = webmercator_to_wgs84(extent[2], extent[3]);
        let center = webmercator_to_wgs84(
            f64::midpoint(extent[0], extent[2]),
            f64::midpoint(extent[1], extent[3]),
        );
        let first_img = images
            .values()
            .next()
            .ok_or_else(|| CogError::NoImagesFound(PathBuf::from(&location)))?;
        let output_format = first_img.output_format().ok_or_else(|| {
            CogError::NotSupportedCompression(
                first_img.compression().to_u16(),
                PathBuf::from(&location),
            )
        })?;

        let mut tilejson = tilejson! {
            tiles: vec![],
            bounds: Bounds::new(
                min.0,
                min.1,
                max.0,
                max.1,
            ),
            center: Center{
                longitude: center.0,
                latitude: center.1,
                zoom: u8::midpoint(max_zoom, min_zoom),
            },
            minzoom: min_zoom,
            maxzoom: max_zoom,
        };
        tilejson
            .other
            .insert("tileSize".to_owned(), Value::from(tile_size));
        tilejson
            .other
            .insert("format".to_owned(), Value::from(output_format.to_string()));

        Ok(Self {
            id,
            location,
            reader,
            min_zoom,
            max_zoom,
            images,
            tilejson,
            tileinfo: TileInfo::new(output_format, Encoding::Internal),
            cache_zoom,
        })
    }
}

/// Find a zoom level of [WebMercatorQuad](https://docs.ogc.org/is/17-083r2/17-083r2.html#72) that
/// is within the error tolerance difference from expected `WebMercatorQuad` zoom levels.
fn web_mercator_zoom(model_resolution: f64, tile_size: u32) -> Option<u8> {
    for z in 0..=MAX_ZOOM {
        let resolution_in_web_mercator =
            EARTH_CIRCUMFERENCE / f64::from(1_u32 << z) / f64::from(tile_size);
        let threshold =
            MAX_ABSOLUTE_RESOLUTION_ERROR.min(resolution_in_web_mercator * MAX_RESOLUTION_ERROR);
        if (model_resolution - resolution_in_web_mercator).abs() < threshold {
            return Some(z);
        }
    }

    None
}

#[async_trait]
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

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    /// Whether this [`Source`] benefits from concurrency when being scraped via `martin-cp`.
    ///
    /// If this returns `true`, martin-cp will suggest concurrent scraping.
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
            tile.z = xyz.z,
            tile.x = xyz.x,
            tile.y = xyz.y,
        ),
        err(Debug),
    )]
    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        if xyz.z < self.min_zoom || xyz.z > self.max_zoom {
            return Ok(Vec::new());
        }
        let image = self.images.get(&(xyz.z)).ok_or_else(|| {
            CogError::ZoomOutOfRange(
                xyz.z,
                PathBuf::from(&self.location),
                self.min_zoom,
                self.max_zoom,
            )
        })?;
        image
            .get_tile(&self.reader, xyz, &self.location)
            .await
            .map_err(Into::into)
    }
}

fn verify_requirements(
    ifd: &ImageFileDirectory,
    model: &ModelInfo,
    path: &Path,
) -> Result<(), CogError> {
    // see requirement 2 in https://docs.ogc.org/is/21-026/21-026.html#_tiles
    if ifd.tile_width().is_none()
        || ifd.tile_height().is_none()
        || ifd.tile_offsets().is_none()
        || ifd.tile_byte_counts().is_none()
    {
        return Err(CogError::NotSupportedChunkType(path.to_path_buf()));
    }

    // see note https://docs.ogc.org/is/21-026/21-026.html#_planar_configuration_considerations
    if ifd.planar_configuration() != PlanarConfiguration::Chunky {
        return Err(CogError::PlanarConfigurationNotSupported(
            path.to_path_buf(),
            0,
            ifd.planar_configuration().to_u16(),
        ));
    }

    let bits = ifd.bits_per_sample();
    let valid_color = bits.iter().all(|bits| *bits == 8)
        && matches!(
            (ifd.photometric_interpretation(), ifd.samples_per_pixel()),
            (PhotometricInterpretation::RGB, 3 | 4) | (PhotometricInterpretation::YCbCr, 3)
        );
    if !valid_color {
        return Err(CogError::InvalidGeoInformation(
            path.to_path_buf(),
            format!(
                "Unsupported color layout {:?}, {} samples, {bits:?} bits per sample",
                ifd.photometric_interpretation(),
                ifd.samples_per_pixel()
            ),
        ));
    }

    if !matches!(
        ifd.compression(),
        Compression::ModernJPEG
            | Compression::Deflate
            | Compression::OldDeflate
            | Compression::LZW
            | Compression::None
            | Compression::WebP
    ) {
        return Err(CogError::NotSupportedCompression(
            ifd.compression().to_u16(),
            path.to_path_buf(),
        ));
    }

    match (&model.pixel_scale, &model.tie_points, &model.transformation) {
        (Some(pixel_scale), Some(tie_points), _)
             =>
        {
            if pixel_scale.len() != 3 {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The count of pixel scale should be 3".to_owned()))
            }
            else if (pixel_scale[0].abs() - pixel_scale[1].abs()).abs() > 0.01{
                Err(CogError::NonSquaredImage(path.to_path_buf(), pixel_scale[0], pixel_scale[1]))
            }
            else if tie_points.len() % 6 != 0 {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The count of tie points should be a multiple of 6".to_owned()))
            }else{
                Ok(())
            }
       }
        (_, _, Some(matrix))
        => {
            if matrix.len() == 16 {
                Ok(())
            } else {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The length of matrix should be 16".to_owned()))
            }
        },
            _ => Err(CogError::InvalidGeoInformation(path.to_path_buf(), "Either a valid transformation (tag 34264) or both pixel scale (tag 33550) and tie points (tag 33922) must be provided".to_owned())),
    }?;

    if model.projected_crs.is_none_or(|crs| crs != 3857u16) {
        return Err(CogError::InvalidGeoInformation(
            path.to_path_buf(),
            "The projected coordinate reference system must be EPSG:3857".to_owned(),
        ));
    }

    Ok(())
}

fn get_image(
    ifd: Arc<ImageFileDirectory>,
    path: &Path,
    origin: [f64; 3],
    resolution: f64,
) -> Result<Image, CogError> {
    let tile_size = ifd
        .tile_width()
        .ok_or_else(|| CogError::NotSupportedChunkType(path.to_path_buf()))?;
    if ifd.tile_height() != Some(tile_size) {
        return Err(CogError::InconsistentTiling(path.to_path_buf()));
    }
    let (image_width, image_length) = (ifd.image_width(), ifd.image_height());
    let zoom_level = web_mercator_zoom(resolution, tile_size)
        .ok_or(CogError::UnknownZoomLevel(path.to_path_buf()))?;
    let ideal_resolution =
        EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom_level) / f64::from(tile_size);
    let tiles_origin = get_tiles_origin(tile_size, ideal_resolution, [origin[0], origin[1]])
        .ok_or(CogError::GetOriginFailed(path.to_path_buf()))?;
    let tiles_across = image_width.div_ceil(tile_size);
    let tiles_down = image_length.div_ceil(tile_size);

    Ok(Image::new(
        zoom_level,
        tiles_origin,
        tiles_across,
        tiles_down,
        tile_size,
        ifd.compression(),
        ifd.samples_per_pixel(),
        ifd,
    ))
}

/// Calculates the origin of the first tile
fn get_tiles_origin(tile_size: u32, resolution: f64, origin: [f64; 2]) -> Option<(u32, u32)> {
    let tile_size_mercator_metres = f64::from(tile_size) * resolution;
    let xf = ((origin[0] + (EARTH_CIRCUMFERENCE / 2.0)) / tile_size_mercator_metres).round();
    let yf = (((EARTH_CIRCUMFERENCE / 2.0) - origin[1]) / tile_size_mercator_metres).round();
    #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let tile_origin_x =
        (xf.is_finite() && xf >= 0.0 && xf <= f64::from(u32::MAX)).then_some(xf as u32)?;
    #[expect(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
    let tile_origin_y =
        (yf.is_finite() && yf >= 0.0 && yf <= f64::from(u32::MAX)).then_some(yf as u32)?;

    Some((tile_origin_x, tile_origin_y))
}

/// Converts pixel dimensions to model space dimensions using resolution values
fn dimensions_in_model(
    ifd: &ImageFileDirectory,
    path: &Path,
    pixel_scale: Option<&[f64]>,
    transformation: Option<&[f64]>,
) -> Result<(f64, f64), CogError> {
    let (image_width_pixel, image_length_pixel) = (ifd.image_width(), ifd.image_height());

    let full_resolution = get_full_resolution(pixel_scale, transformation, path)?;

    let width_in_model = f64::from(image_width_pixel) * full_resolution[0].abs();
    let length_in_model = f64::from(image_length_pixel) * full_resolution[1].abs();

    Ok((width_in_model, length_in_model))
}

fn get_origin(
    tie_points: Option<&[f64]>,
    transformation: Option<&[f64]>,
    path: &Path,
) -> Result<[f64; 3], CogError> {
    // From geotiff spec: "This matrix tag should not be used if the ModelTiepointTag and the ModelPixelScaleTag are already defined"
    // See more in https://docs.ogc.org/is/19-008r4/19-008r4.html#_geotiff_tags_for_coordinate_transformations
    match (tie_points, transformation) {
        // From geotiff spec: "If possible, the first tiepoint placed in this tag shall be the one establishing the location of the point (0,0) in raster space"
        (Some(points), _) if points.len() >= 6 => Ok([points[3], points[4], points[5]]),

        // coords =     matrix  * coords
        // |- -|     |-       -|  |- -|
        // | X |     | a b c d |  | I |
        // | | |     |         |  |   |
        // | Y |     | e f g h |  | J |
        // |   |  =  |         |  |   |
        // | Z |     | i j k l |  | K |
        // | | |     |         |  |   |
        // | 1 |     | m n o p |  | 1 |
        // |- -|     |-       -|  |- -|

        // The (I,J,K) of origin is (0,0,0), so:
        //
        //    x = I*a + J*b + K*c + 1*d => d => matrix[3]
        //    y = I*e + J*f + k*g + 1*h => h => matrix[7]
        //    z = I*i + J*j + K*k + 1*l => l => matrix[11]
        (_, Some(matrix)) if matrix.len() >= 12 => Ok([matrix[3], matrix[7], matrix[11]]),
        _ => Err(CogError::GetOriginFailed(path.to_path_buf())),
    }
}

fn get_full_resolution(
    pixel_scale: Option<&[f64]>,
    transformation: Option<&[f64]>,
    path: &Path,
) -> Result<[f64; 2], CogError> {
    match (pixel_scale, transformation) {
        // ModelPixelScaleTag = (ScaleX, ScaleY, ScaleZ)
        (Some(scale), _) => Ok([scale[0], scale[1]]),
        // here we adopted the 2-d matrix form based on the geotiff spec, the z-axis is dropped intentionally, see https://docs.ogc.org/is/19-008r4/19-008r4.html#_geotiff_tags_for_coordinate_transformations
        // It looks like this:
        /*
           |- -|   |-       -| |- -|
           | X |   | a b 0 d | | I |
           | | |   |         | |   |
           | Y |   | e f 0 h | | J |
           |   | = |         | |   |
           | Z |   | 0 0 0 0 | | K |
           | | |   |         | |   |
           | 1 |   | 0 0 0 1 | | 1 |
           |- -|   |-       -| |- -|
        */
        (_, Some(matrix)) => {
            let mut x_res = matrix[0].hypot(matrix[4]);
            x_res = x_res.copysign(matrix[0]);
            let mut y_res = matrix[1].hypot(matrix[5]);
            // A positive y_res indicates that model space Y coordinates decrease as raster space J indices increase. This is the standard vertical relationship between raster space and model space
            y_res = y_res.copysign(-matrix[5]);
            Ok([x_res, y_res]) // drop the z scale directly as we don't use it
        }
        (None, None) => Err(CogError::GetFullResolutionFailed(path.to_path_buf())),
    }
}

fn raster2model(i: u32, j: u32, matrix: &[f64]) -> (f64, f64) {
    let i = f64::from(i);
    let j = f64::from(j);
    let x = matrix[1].mul_add(j, matrix[0].mul_add(i, matrix[3]));
    let y = matrix[5].mul_add(j, matrix[4].mul_add(i, matrix[7]));
    (x, y)
}

/// Computes the bounding box (`[min_x, min_y, max_x, max_y]`) based on the transformation matrix, origin, width, and height.
fn get_extent(
    origin: &[f64; 3],
    transformation: Option<&[f64]>,
    (full_width_pixel, full_height_pixel): (u32, u32),
    (full_width, full_height): (f64, f64),
) -> [f64; 4] {
    if let Some(matrix) = transformation {
        let corner_pixels = [
            (0, 0),                                // Top-left
            (0, full_height_pixel),                // Bottom-left
            (full_width_pixel, 0),                 // Top-right
            (full_width_pixel, full_height_pixel), // Bottom-right
        ];

        // Transform the first corner to initialize min/max values
        let (mut min_x, mut min_y) = raster2model(corner_pixels[0].0, corner_pixels[0].1, matrix);
        let mut max_x = min_x;
        let mut max_y = min_y;

        // Iterate over the rest of the corners
        for &(i, j) in corner_pixels.iter().skip(1) {
            let (x, y) = raster2model(i, j, matrix);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
        return [min_x, min_y, max_x, max_y];
    }
    let [x1, y1, _] = origin;
    let x2 = x1 + full_width;
    let y2 = y1 - full_height;

    [x1.min(x2), y1.min(y2), x1.max(x2), y1.max(y2)]
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    #[cfg(target_os = "linux")]
    use std::ffi::OsStr;
    #[cfg(target_os = "linux")]
    use std::os::unix::ffi::OsStrExt as _;

    use approx::assert_abs_diff_eq;
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

        assert!(matches!(result, Err(CogError::AsyncTiff(_, _))));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn opens_a_local_cog_with_a_non_utf8_filename() {
        let temp =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("../target")).unwrap();
        let path = temp.path().join(OsStr::from_bytes(b"image-\xff.tif"));
        std::fs::copy("../tests/fixtures/cog/usda_naip_128_none_z2.tif", &path).unwrap();

        let source = CogSource::new("non-utf8".to_owned(), path, CacheZoomRange::default())
            .await
            .unwrap();
        let tile = source
            .get_tile(
                TileCoord {
                    z: 18,
                    x: 42_712,
                    y: 97_343,
                },
                None,
            )
            .await
            .unwrap();

        assert!(!tile.is_empty());
    }

    #[tokio::test]
    async fn opens_a_cog_with_more_than_32_kib_of_metadata() {
        let source = CogSource::new(
            "large-metadata".to_owned(),
            Path::new("../tests/fixtures/cog/regressions/usda_naip_128_large_metadata.tif")
                .to_path_buf(),
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
            Path::new("../tests/fixtures/cog/regressions/usda_naip_128_none_sparse.tif")
                .to_path_buf(),
            CacheZoomRange::default(),
        )
        .await
        .unwrap();

        let tile = source
            .get_tile(
                TileCoord {
                    z: 19,
                    x: 85_424,
                    y: 194_685,
                },
                None,
            )
            .await
            .unwrap();
        assert!(tile.is_empty());
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
        longitude: -121.343_650_817_871_05,
        latitude: 41.968_680_268_127_26,
        zoom: 18,
    }, Bounds {
        left: -121.343_994_140_624_97,
        top: 41.969_190_794_214_65,
        right: -121.343_307_495_117_16,
        bottom: 41.968_169_737_948_43,
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
        let source = CogSource::new(
            cog_file,
            Path::new(&path).to_path_buf(),
            CacheZoomRange::default(),
        )
        .await
        .unwrap();

        assert_eq!(source.max_zoom, max_zoom);
        assert_eq!(source.min_zoom, min_zoom);
        assert_eq!(
            source.tilejson.center.unwrap().to_string(),
            center.to_string()
        );
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

    #[rstest]
    #[case(
        Some(vec![0.0, 0.0, 0.0, 1_620_750.250_8, 4_277_012.715_3, 0.0]),None,
        Some([1_620_750.250_8, 4_277_012.715_3, 0.0])
    )]
    #[case(
        None,Some(vec![
            0.0, 100.0, 0.0, 400_000.0, 100.0, 0.0, 0.0, 500_000.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
            0.0, 1.0,
        ]),
        Some([400_000.0, 500_000.0, 0.0])
    )]
    #[case(None, None, None)]
    fn can_get_origin(
        #[case] tie_point: Option<Vec<f64>>,
        #[case] matrix: Option<Vec<f64>>,
        #[case] expected: Option<[f64; 3]>,
    ) {
        use approx::assert_abs_diff_eq;

        let origin = super::get_origin(
            tie_point.as_deref(),
            matrix.as_deref(),
            Path::new("not_exist.tif"),
        )
        .ok();
        match (origin, expected) {
            (Some(o), Some(e)) => {
                assert_abs_diff_eq!(o[0], e[0]);
                assert_abs_diff_eq!(o[1], e[1]);
                assert_abs_diff_eq!(o[2], e[2]);
            }
            (None, None) => {
                // Both are None, which is expected
            }
            _ => {
                panic!("Origin {origin:?} does not match expected {expected:?}");
            }
        }
    }

    #[rstest]
    #[case(
        None,Some(vec![10.0, 10.0,0.0]),Some(vec![0.0, 0.0, 0.0, 1_620_750.250_8, 4_277_012.715_3, 0.0]),(512,512),
        [1_620_750.250_8, 4_271_892.715_3, 1_625_870.250_8, 4_277_012.715_3]
    )]
    #[case(
        Some(vec![
            10.0,0.0,0.0,1_620_750.250_8,
            0.0,-10.0,0.0,4_277_012.715_3,
            0.0,0.0,0.0,0.0,
            0.0,0.0,0.0,1.0
        ]),None,None,(512,512),
        [1_620_750.250_8, 4_271_892.715_3, 1_625_870.250_8, 4_277_012.715_3]
    )]
    #[case(
        Some(vec![
            0.010_005_529_647_693, 0.0, 0.0, -7.583_906_932_854_38,
            0.0, -0.009_986_188_755_447_6, 0.0, 38.750_354_738_325_9,
            0.0, 0.0, 0.0, 0.0,
            0.0, 0.0, 0.0, 1.0
        ]), None, None, (598, 279),
        [-7.583_906_9, 35.964_208_1, -1.600_600_2, 38.750_354_7]
    )]
    fn can_get_extent(
        #[case] matrix: Option<Vec<f64>>,
        #[case] pixel_scale: Option<Vec<f64>>,
        #[case] tie_point: Option<Vec<f64>>,
        #[case] (full_width_pixel, full_length_pixel): (u32, u32),
        #[case] expected_extent: [f64; 4],
    ) {
        use approx::assert_abs_diff_eq;

        use crate::tiles::cog::source::{get_extent, get_full_resolution, get_origin};

        let origin = get_origin(
            tie_point.as_deref(),
            matrix.as_deref(),
            Path::new("not_exist.tif"),
        )
        .unwrap();
        let full_resolution = get_full_resolution(
            pixel_scale.as_deref(),
            matrix.as_deref(),
            Path::new("not_exist.tif"),
        )
        .unwrap();

        let full_width = full_resolution[0] * f64::from(full_width_pixel);
        let full_length = full_resolution[1] * f64::from(full_length_pixel);

        let extent = get_extent(
            &origin,
            matrix.as_deref(),
            (full_width_pixel, full_length_pixel),
            (full_width, full_length),
        );

        assert_abs_diff_eq!(extent[0], expected_extent[0], epsilon = 0.00001);
        assert_abs_diff_eq!(extent[1], expected_extent[1], epsilon = 0.00001);
        assert_abs_diff_eq!(extent[2], expected_extent[2], epsilon = 0.00001);
        assert_abs_diff_eq!(extent[3], expected_extent[3], epsilon = 0.00001);
    }

    #[rstest]
    #[case(
        None,Some(vec![118.450_587_6, 118.450_587_6, 0.0]), [118.450_587_6, 118.450_587_6]
    )]
    #[case(
        None,Some(vec![100.00, -100.0]), [100.0, -100.0]
    )]
    #[
        case(
            Some(vec![
                0.010_005_529_647_693_3, 0.0, 0.0, -7.583_906_932_854_38, 0.0, -0.009_986_188_755_447_63, 0.0, 38.750_354_738_325_9, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
            None, [0.010_005_529_647_693, 0.009_986_188_755_448])
    ]
    fn can_get_full_resolution(
        #[case] matrix: Option<Vec<f64>>,
        #[case] pixel_scale: Option<Vec<f64>>,
        #[case] expected: [f64; 2],
    ) {
        use approx::assert_abs_diff_eq;

        use crate::tiles::cog::source::get_full_resolution;

        let full_resolution = get_full_resolution(
            pixel_scale.as_deref(),
            matrix.as_deref(),
            Path::new("not_exist.tif"),
        )
        .unwrap();
        assert_abs_diff_eq!(full_resolution[0], expected[0], epsilon = 0.00001);
        assert_abs_diff_eq!(full_resolution[1], expected[1], epsilon = 0.00001);
    }

    #[rstest]
    #[case(156_543.033_928_041_03, 256, Some(0))]
    #[case(78_271.516_964_020_51, 256, Some(1))]
    #[case(39_135.758_482_010_26, 256, Some(2))]
    #[case(19_567.879_241_005_13, 256, Some(3))]
    #[case(78_271.516_964_020_51, 512, Some(0))]
    #[case(39_135.758_482_010_26, 512, Some(1))]
    #[case(19_567.879_241_005_13, 512, Some(2))]
    #[case(9_783.939_620_502_564, 512, Some(3))]
    #[case(39_135.758_482_010_26, 1024, Some(0))]
    #[case(19_567.879_241_005_13, 1024, Some(1))]
    #[case(9_783.939_620_502_564, 1024, Some(2))]
    #[case(4_891.969_810_251_282, 1024, Some(3))]
    fn can_get_web_mercator_zoom(
        #[case] resolution: f64,
        #[case] tile_size: u32,
        #[case] expected_zoom: Option<u8>,
    ) {
        use crate::tiles::cog::source::web_mercator_zoom;
        assert_eq!(web_mercator_zoom(resolution, tile_size), expected_zoom);
    }
}
