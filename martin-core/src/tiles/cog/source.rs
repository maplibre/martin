use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::vec;

use async_trait::async_trait;
use martin_tile_utils::{
    EARTH_CIRCUMFERENCE, Format, MAX_ZOOM, TileCoord, TileData, TileInfo, webmercator_to_wgs84,
};
use serde_json::Value;
use tiff::decoder::{ChunkType, Decoder};
use tiff::tags::Tag::{self};
use tiff::tags::{CompressionMethod, PlanarConfiguration};
use tilejson::{Bounds, Center, TileJSON, tilejson};

use crate::tiles::cog::CogError;
use crate::tiles::cog::image::Image;
use crate::tiles::cog::model::ModelInfo;
use crate::tiles::{MartinCoreResult, Source, UrlQuery};

/// Maximum allowed difference from a matching WebMercatorQuad tile matrix zoom level.
pub const MAX_RESOLUTION_ERROR: f64 = 1e-12;

/// Tile source that reads from `Cloud Optimized GeoTIFF` files.
#[derive(Clone, Debug)]
pub struct CogSource {
    id: String,
    path: PathBuf,
    min_zoom: u8,
    max_zoom: u8,
    images: HashMap<u8, Image>,
    tilejson: TileJSON,
    tileinfo: TileInfo,
}

impl CogSource {
    /// Creates a new COG tile source from a file path.
    #[allow(clippy::too_many_lines)]
    pub fn new(id: String, path: PathBuf) -> Result<Self, CogError> {
        let tif_file =
            File::open(&path).map_err(|e: std::io::Error| CogError::IoError(e, path.clone()))?;
        let mut decoder = Decoder::new(tif_file)
            .map_err(|e| CogError::InvalidTiffFile(e, path.clone()))?
            .with_limits(tiff::decoder::Limits::default());
        let model = ModelInfo::decode(&mut decoder, &path);
        verify_requirements(&mut decoder, &model, &path.clone())?;
        let origin = get_origin(
            model.tie_points.as_deref(),
            model.transformation.as_deref(),
            &path,
        )?;
        let (full_width_pixel, full_length_pixel) = dimensions_in_pixel(&mut decoder, &path, 0)?;
        let (full_width, full_length) = dimensions_in_model(
            &mut decoder,
            &path,
            0,
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
        let mut ifd_index = 0;

        loop {
            if !decoder.more_images() {
                break;
            }
            if decoder.seek_to_image(ifd_index).is_err() {
                break;
            }

            let subfile_type_tag = decoder.get_tag_u32(Tag::NewSubfileType);
            let is_source_image = subfile_type_tag.is_err();
            let is_reduced_resolution_subfile =
                subfile_type_tag.map_or_else(|_| false, |v| v == 0b001);
            if is_source_image || is_reduced_resolution_subfile {
                let image_width = dimensions_in_pixel(&mut decoder, &path, ifd_index)?.0;
                let resolution = full_width / f64::from(image_width);
                images.push(get_image(
                    &mut decoder,
                    &path,
                    ifd_index,
                    origin,
                    resolution,
                )?);
            }

            ifd_index += 1;
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
                        Err(CogError::InconsistentTiling(path.clone()))?;
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
            .ok_or_else(|| CogError::NoImagesFound(path.clone()))?;
        let max_zoom = *images
            .keys()
            .max()
            .ok_or_else(|| CogError::NoImagesFound(path.clone()))?;
        let min = webmercator_to_wgs84(extent[0], extent[1]);
        let max = webmercator_to_wgs84(extent[2], extent[3]);
        let center =
            webmercator_to_wgs84((extent[0] + extent[2]) / 2.0, (extent[1] + extent[3]) / 2.0);
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
                zoom: (max_zoom + min_zoom) / 2,
            },
            minzoom: min_zoom,
            maxzoom: max_zoom,
        };
        tilejson
            .other
            .insert("tileSize".to_string(), Value::from(tile_size));
        Ok(CogSource {
            id,
            path,
            min_zoom,
            max_zoom,
            images,
            tilejson,
            tileinfo: TileInfo::new(Format::Png, martin_tile_utils::Encoding::Internal),
        })
    }
}

/// Find a zoom level of [WebMercatorQuad](https://docs.ogc.org/is/17-083r2/17-083r2.html#72) that
/// is within the error tolerance difference from expected WebMercatorQuad zoom levels.
fn web_mercator_zoom(model_resolution: f64, tile_size: u32) -> Option<u8> {
    for z in 0..=MAX_ZOOM {
        let resolution_in_web_mercator =
            EARTH_CIRCUMFERENCE / f64::from(1_u32 << z) / f64::from(tile_size);
        if (model_resolution - resolution_in_web_mercator).abs() < MAX_RESOLUTION_ERROR {
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
        // if we copy from one local file to another, we are likely not bottlenecked by CPU
        // TODO: benchmark this assumption, decoding might be a bottleneck
        false
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        if xyz.z < self.min_zoom || xyz.z > self.max_zoom {
            return Ok(Vec::new());
        }
        let image = self.images.get(&(xyz.z)).ok_or_else(|| {
            CogError::ZoomOutOfRange(xyz.z, self.path.clone(), self.min_zoom, self.max_zoom)
        })?;

        let file = File::open(&self.path).map_err(|e| CogError::IoError(e, self.path.clone()))?;
        let mut decoder = Decoder::new(file)
            .map_err(|e| CogError::InvalidTiffFile(e, self.path.clone()))?
            .with_limits(tiff::decoder::Limits::default());
        let bytes = image.get_tile(&mut decoder, xyz, &self.path)?;
        Ok(bytes)
    }
}

fn verify_requirements(
    decoder: &mut Decoder<File>,
    model: &ModelInfo,
    path: &Path,
) -> Result<(), CogError> {
    // see requirement 2 in https://docs.ogc.org/is/21-026/21-026.html#_tiles
    if decoder.get_chunk_type() != ChunkType::Tile {
        Err(CogError::NotSupportedChunkType(path.to_path_buf()))?;
    }

    // see note https://docs.ogc.org/is/21-026/21-026.html#_planar_configuration_considerations
    decoder
        .get_tag_unsigned(Tag::PlanarConfiguration)
        .map_err(|e| {
            CogError::TagsNotFound(
                e,
                vec![Tag::PlanarConfiguration.to_u16()],
                0,
                path.to_path_buf(),
            )
        })
        .and_then(|config| {
            if config == PlanarConfiguration::Chunky.to_u16() {
                Ok(())
            } else {
                Err(CogError::PlanarConfigurationNotSupported(
                    path.to_path_buf(),
                    0,
                    config,
                ))
            }
        })?;

    decoder
        .colortype()
        .map_err(|e| CogError::InvalidTiffFile(e, path.to_path_buf()))
        .and_then(|color_type| {
            if matches!(
                color_type,
                tiff::ColorType::RGB(8) | tiff::ColorType::RGBA(8)
            ) {
                Ok(())
            } else {
                Err(CogError::NotSupportedColorTypeAndBitDepth(
                    color_type,
                    path.to_path_buf(),
                ))
            }
        })?;

    decoder
        .get_tag_unsigned(Tag::Compression)
        .map_err(|e| {
            CogError::TagsNotFound(e, vec![Tag::Compression.to_u16()], 0, path.to_path_buf())
        })
        .and_then(|compression: u16| {
            if matches! {
                CompressionMethod::from_u16(compression),
                Some(CompressionMethod::None) |
                Some(CompressionMethod::LZW) |
                Some(CompressionMethod::Deflate)
            } {
                Ok(())
            } else {
                Err(CogError::NotSupportedCompression(
                    compression,
                    path.to_path_buf(),
                ))
            }
        })?;

    match (&model.pixel_scale, &model.tie_points, &model.transformation) {
        (Some(pixel_scale), Some(tie_points), _)
             =>
        {
            if pixel_scale.len() != 3 {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The count of pixel scale should be 3".to_string()))
            }
            else if (pixel_scale[0].abs() - pixel_scale[1].abs()).abs() > 0.01{
                Err(CogError::NonSquaredImage(path.to_path_buf(), pixel_scale[0], pixel_scale[1]))
            }
            else if tie_points.len() % 6 != 0 {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The count of tie points should be a multiple of 6".to_string()))
            }else{
                Ok(())
            }
       }
        (_, _, Some(matrix))
        => {
            if matrix.len() == 16 {
                Ok(())
            } else {
                Err(CogError::InvalidGeoInformation(path.to_path_buf(), "The length of matrix should be 16".to_string()))
            }
        },
            _ => Err(CogError::InvalidGeoInformation(path.to_path_buf(), "Either a valid transformation (tag 34264) or both pixel scale (tag 33550) and tie points (tag 33922) must be provided".to_string())),
    }?;

    if model.projected_crs.is_none_or(|crs| crs != 3857u16) {
        return Err(CogError::InvalidGeoInformation(
            path.to_path_buf(),
            "The projected coordinate reference system must be EPSG:3857".to_string(),
        ));
    }

    Ok(())
}

fn get_image(
    decoder: &mut Decoder<File>,
    path: &Path,
    ifd_index: usize,
    origin: [f64; 3],
    resolution: f64,
) -> Result<Image, CogError> {
    let tile_size = decoder.chunk_dimensions().0;
    let (image_width, image_length) = dimensions_in_pixel(decoder, path, ifd_index)?;
    let zoom_level = web_mercator_zoom(resolution, tile_size)
        .ok_or(CogError::GetOriginFailed(path.to_path_buf()))?;
    let tiles_origin = get_tiles_origin(tile_size, resolution, [origin[0], origin[1]])
        .ok_or(CogError::GetOriginFailed(path.to_path_buf()))?;
    let tiles_across = image_width.div_ceil(tile_size);
    let tiles_down = image_length.div_ceil(tile_size);
    Ok(Image::new(
        ifd_index,
        zoom_level,
        tiles_origin,
        tiles_across,
        tiles_down,
        tile_size,
    ))
}

/// Calculates the origin of the first tile
fn get_tiles_origin(tile_size: u32, resolution: f64, origin: [f64; 2]) -> Option<(u32, u32)> {
    let tile_size_mercator_metres = f64::from(tile_size) * resolution;
    let tile_origin_x_f = (origin[0] + (EARTH_CIRCUMFERENCE / 2.0)) / tile_size_mercator_metres;
    let tile_origin_y_f = ((EARTH_CIRCUMFERENCE / 2.0) - origin[1]) / tile_size_mercator_metres;
    let tile_origin_x = tile_origin_x_f.floor() as u32;
    let tile_origin_y = tile_origin_y_f.floor() as u32;

    Some((tile_origin_x, tile_origin_y))
}

/// Gets image pixel dimensions from TIFF decoder
fn dimensions_in_pixel(
    decoder: &mut Decoder<File>,
    path: &Path,
    ifd_index: usize,
) -> Result<(u32, u32), CogError> {
    let (image_width, image_length) = decoder.dimensions().map_err(|e| {
        CogError::TagsNotFound(
            e,
            vec![Tag::ImageWidth.to_u16(), Tag::ImageLength.to_u16()],
            ifd_index,
            path.to_path_buf(),
        )
    })?;

    Ok((image_width, image_length))
}

/// Converts pixel dimensions to model space dimensions using resolution values
fn dimensions_in_model(
    decoder: &mut Decoder<File>,
    path: &Path,
    ifd_index: usize,
    pixel_scale: Option<&[f64]>,
    transformation: Option<&[f64]>,
) -> Result<(f64, f64), CogError> {
    let (image_width_pixel, image_length_pixel) = dimensions_in_pixel(decoder, path, ifd_index)?;

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
            let mut x_res = (matrix[0] * matrix[0] + matrix[4] * matrix[4]).sqrt();
            x_res = x_res.copysign(matrix[0]);
            let mut y_res = (matrix[1] * matrix[1] + matrix[5] * matrix[5]).sqrt();
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
    let x = matrix[3] + (matrix[0] * i) + (matrix[1] * j);
    let y = matrix[7] + (matrix[4] * i) + (matrix[5] * j);
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
    use crate::tiles::cog::CogSource;
    use rstest::rstest;
    use std::path::Path;
    use tilejson::{Bounds, Center};

    #[rstest]
    #[case("usda_naip_256_lzw_z3".to_string(), Center {
        longitude: -121.34674072265622,
        latitude: 41.96765920367816,
        zoom: 17,
    }, Bounds {
        left: -121.34948730468746,
        top: 41.97174336327965,
        right: -121.34399414062497,
        bottom: 41.96357478222515,
    }, 16, 18, 256)]
    #[case("usda_naip_512_deflate_z2".to_string(), Center {
        longitude: -121.34674072265622,
        latitude: 41.96765920367816,
        zoom: 16,
    }, Bounds {
        left: -121.34948730468746,
        top: 41.97174336327965,
        right: -121.34399414062497,
        bottom: 41.96357478222515,
    }, 16, 17, 512)]
    fn can_generate_tilejson_from_source(
        #[case] cog_file: String,
        #[case] center: Center,
        #[case] bounds: Bounds,
        #[case] min_zoom: u8,
        #[case] max_zoom: u8,
        #[case] tile_size: u32,
    ) {
        let path = format!("../tests/fixtures/cog/{}.tif", cog_file);
        let source = CogSource::new(cog_file, Path::new(&path).to_path_buf()).unwrap();

        assert_eq!(source.max_zoom, max_zoom);
        assert_eq!(source.min_zoom, min_zoom);
        assert_eq!(
            source.tilejson.center.unwrap().to_string(),
            center.to_string()
        );
        assert_eq!(
            source.tilejson.bounds.unwrap().to_string(),
            bounds.to_string()
        );
        assert_eq!(source.tilejson.other.get("tileSize").unwrap(), tile_size);
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
    #[case(156543.03392804103, 256, Some(0))]
    #[case(78271.51696402051, 256, Some(1))]
    #[case(39135.75848201026, 256, Some(2))]
    #[case(19567.87924100513, 256, Some(3))]
    #[case(78271.51696402051, 512, Some(0))]
    #[case(39135.75848201026, 512, Some(1))]
    #[case(19567.87924100513, 512, Some(2))]
    #[case(9783.939620502564, 512, Some(3))]
    #[case(39135.75848201026, 1024, Some(0))]
    #[case(19567.87924100513, 1024, Some(1))]
    #[case(9783.939620502564, 1024, Some(2))]
    #[case(4891.969810251282, 1024, Some(3))]
    fn can_get_web_mercator_zoom(
        #[case] resolution: f64,
        #[case] tile_size: u32,
        #[case] expected_zoom: Option<u8>,
    ) {
        use crate::tiles::cog::source::web_mercator_zoom;
        assert_eq!(web_mercator_zoom(resolution, tile_size), expected_zoom);
    }
}
