//! `GeoTIFF` georeferencing: where the image sits in `EPSG:3857`, and how its levels map to zooms.

use async_tiff::ImageFileDirectory;

use crate::tile::{HALF_EARTH, MAX_ZOOM, resolution};
use crate::{CogError, CogResult};

/// `ProjectedCRSGeoKey` values that denote Web Mercator: the EPSG code and its deprecated alias.
/// (The unofficial `900913` does not fit in a 16-bit `GeoKey`.)
const WEB_MERCATOR_CODES: [u16; 2] = [3857, 3785];

/// `GTRasterTypeGeoKey` value for `RasterPixelIsPoint`.
const RASTER_PIXEL_IS_POINT: u16 = 2;

/// How far (as a fraction of a pixel) a level's origin may be from a tile corner.
/// Producers often round coordinates to a millimeter, which is ~1/75 px at z21.
pub(crate) const ALIGNMENT_TOLERANCE_PX: f64 = 0.05;

/// The largest difference between pixel width and height, in meters, still treated as square.
const MAX_NON_SQUARE_DIFFERENCE: f64 = 0.01;

/// Relative tolerance when matching a level's resolution to a zoom level's resolution.
const RESOLUTION_TOLERANCE: f64 = 1e-3;

/// Where the full-resolution image sits in `EPSG:3857`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Georef {
    /// `EPSG:3857` x of the top-left corner of the top-left pixel.
    pub origin_x: f64,
    /// `EPSG:3857` y of the top-left corner of the top-left pixel.
    pub origin_y: f64,
    /// Meters per pixel, the same horizontally and vertically.
    pub resolution: f64,
}

impl Georef {
    /// Reads and validates the georeferencing of the full-resolution image.
    pub fn from_ifd(ifd: &ImageFileDirectory, location: &str) -> CogResult<Self> {
        let unsupported = |reason: String| CogError::unsupported(location, reason);

        let geo_keys = ifd
            .geo_key_directory()
            .ok_or_else(|| unsupported("it has no GeoTIFF GeoKeyDirectory".to_owned()))?;
        match geo_keys.projected_type {
            Some(code) if WEB_MERCATOR_CODES.contains(&code) => {}
            Some(code) => {
                return Err(unsupported(format!(
                    "its CRS is EPSG:{code}, but only EPSG:3857 (Web Mercator) is supported"
                )));
            }
            None => {
                return Err(unsupported(
                    "it has no projected CRS, but only EPSG:3857 (Web Mercator) is supported"
                        .to_owned(),
                ));
            }
        }

        let (mut origin_x, mut origin_y, scale_x, scale_y) = if let (Some(scale), Some(tie)) =
            (ifd.model_pixel_scale(), ifd.model_tiepoint())
        {
            if scale.len() < 2 || tie.len() < 6 {
                return Err(unsupported(
                    "ModelPixelScale or ModelTiepoint has too few values".to_owned(),
                ));
            }
            // The tiepoint maps raster (i, j) to model (x, y); move it to raster (0, 0).
            let (sx, sy) = (scale[0], scale[1]);
            (
                tie[0].mul_add(-sx, tie[3]),
                tie[1].mul_add(sy, tie[4]),
                sx,
                sy,
            )
        } else if let Some(m) = ifd.model_transformation() {
            if m.len() < 8 {
                return Err(unsupported(
                    "ModelTransformation has fewer than 8 values".to_owned(),
                ));
            }
            if m[1] != 0.0 || m[4] != 0.0 {
                return Err(unsupported(
                    "the image is rotated or sheared (ModelTransformation)".to_owned(),
                ));
            }
            (m[3], m[7], m[0], -m[5])
        } else {
            return Err(unsupported(
                "it has neither ModelPixelScale + ModelTiepoint nor ModelTransformation".to_owned(),
            ));
        };

        if !(scale_x.is_finite() && scale_y.is_finite() && scale_x > 0.0 && scale_y > 0.0) {
            return Err(unsupported(format!(
                "pixel size {scale_x} x {scale_y} is not positive (flipped images are not supported)"
            )));
        }
        if (scale_x - scale_y).abs() > MAX_NON_SQUARE_DIFFERENCE {
            return Err(unsupported(format!(
                "pixels are not square ({scale_x} x {scale_y} m)"
            )));
        }
        if geo_keys.raster_type == Some(RASTER_PIXEL_IS_POINT) {
            // The coordinates refer to pixel centers; tiles are aligned on pixel corners.
            origin_x -= scale_x / 2.0;
            origin_y += scale_y / 2.0;
        }
        if !(origin_x.is_finite() && origin_y.is_finite()) {
            return Err(unsupported(
                "the image origin is not a finite number".to_owned(),
            ));
        }

        Ok(Self {
            origin_x,
            origin_y,
            resolution: scale_x,
        })
    }
}

/// Finds the zoom whose `tile_size`-pixel tiles have the given resolution.
pub(crate) fn match_zoom(level_resolution: f64, tile_size: u32) -> Option<u8> {
    (0..=MAX_ZOOM).find(|&z| {
        let expected = resolution(z, tile_size);
        (level_resolution - expected).abs() <= expected * RESOLUTION_TOLERANCE
    })
}

/// The tile column and row whose top-left corner is at `(x, y)` at zoom `z`,
/// if `(x, y)` is within [`ALIGNMENT_TOLERANCE_PX`] of such a corner.
pub(crate) fn aligned_tile(x: f64, y: f64, z: u8, tile_size: u32) -> Option<(u32, u32)> {
    let res = resolution(z, tile_size);
    let tile_meters = res * f64::from(tile_size);
    let col = (x + HALF_EARTH) / tile_meters;
    let row = (HALF_EARTH - y) / tile_meters;
    let tolerance = ALIGNMENT_TOLERANCE_PX / f64::from(tile_size);
    let tiles = f64::from(1_u32 << z);
    let snap = |v: f64| {
        let rounded = v.round();
        ((v - rounded).abs() <= tolerance && (0.0..tiles).contains(&rounded)).then_some(rounded)
    };
    // Both values are integers in 0..2^z, checked above.
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some((snap(col)? as u32, snap(row)? as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_matching() {
        assert_eq!(match_zoom(0.074_645_535_434_742, 256), Some(21));
        assert_eq!(match_zoom(0.149_291_070_869_822, 256), Some(20));
        assert_eq!(match_zoom(0.149_291_070_869_822, 512), Some(19));
        assert_eq!(match_zoom(0.1, 256), None);
    }

    #[test]
    fn alignment() {
        let (x, y) = (-12_229.924_525_629_729, 6_710_559.587_212_194);
        assert_eq!(aligned_tile(x, y, 16, 256), Some((32748, 21794)));
        assert_eq!(aligned_tile(x, y, 21, 256), Some((32748 * 32, 21794 * 32)));
        // A millimeter off is still aligned at z21 (7.5 cm pixels); half a pixel is not.
        assert_eq!(
            aligned_tile(x + 0.001, y, 21, 256),
            Some((32748 * 32, 21794 * 32))
        );
        assert_eq!(aligned_tile(x + 0.037, y, 21, 256), None);
    }
}
