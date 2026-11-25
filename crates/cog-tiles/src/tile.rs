use std::f64::consts::PI;
use std::fmt;
use std::str::FromStr;

use crate::{CogError, CogResult};

/// The largest supported zoom level.
pub const MAX_ZOOM: u8 = 30;

/// Circumference of the Earth at the equator in `EPSG:3857` meters.
pub const EARTH_CIRCUMFERENCE: f64 = 40_075_016.685_578_49;

/// Half of [`EARTH_CIRCUMFERENCE`]: the `EPSG:3857` coordinate of the world's east and north edges.
pub const HALF_EARTH: f64 = EARTH_CIRCUMFERENCE / 2.0;

/// A `WebMercatorQuad` (XYZ, "slippy map") tile coordinate, with `y = 0` at the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileCoord {
    z: u8,
    x: u32,
    y: u32,
}

impl TileCoord {
    /// Creates a tile coordinate, validating that `x` and `y` exist at zoom `z`.
    ///
    /// # Errors
    /// Returns [`CogError::InvalidTileCoord`] if `z > MAX_ZOOM` or `x`/`y` is out of range.
    pub fn new(z: u8, x: u32, y: u32) -> CogResult<Self> {
        if z > MAX_ZOOM || u64::from(x) >= 1_u64 << z || u64::from(y) >= 1_u64 << z {
            return Err(CogError::InvalidTileCoord { z, x, y });
        }
        Ok(Self { z, x, y })
    }

    /// Creates a tile coordinate the caller knows to be valid.
    pub(crate) fn new_unchecked(z: u8, x: u32, y: u32) -> Self {
        debug_assert!(Self::new(z, x, y).is_ok(), "{z}/{x}/{y} is not a valid tile");
        Self { z, x, y }
    }

    /// Zoom level.
    #[must_use]
    pub fn z(&self) -> u8 {
        self.z
    }

    /// Column, counting from the west.
    #[must_use]
    pub fn x(&self) -> u32 {
        self.x
    }

    /// Row, counting from the north.
    #[must_use]
    pub fn y(&self) -> u32 {
        self.y
    }

    /// The tile's extent in `EPSG:3857` meters as `[min_x, min_y, max_x, max_y]`.
    #[must_use]
    pub fn mercator_bounds(&self) -> [f64; 4] {
        let size = tile_extent(self.z);
        let min_x = f64::from(self.x).mul_add(size, -HALF_EARTH);
        let max_y = f64::from(self.y).mul_add(-size, HALF_EARTH);
        [min_x, max_y - size, min_x + size, max_y]
    }
}

impl fmt::Display for TileCoord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.z, self.x, self.y)
    }
}

impl FromStr for TileCoord {
    type Err = CogError;

    /// Parses a `z/x/y` string.
    fn from_str(value: &str) -> CogResult<Self> {
        let invalid = || CogError::InvalidTileCoord { z: u8::MAX, x: 0, y: 0 };
        let mut parts = value.split('/');
        let (Some(z), Some(x), Some(y), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
            return Err(invalid());
        };
        Self::new(
            z.parse().map_err(|_err| invalid())?,
            x.parse().map_err(|_err| invalid())?,
            y.parse().map_err(|_err| invalid())?,
        )
    }
}

/// The side of one tile at zoom `z`, in `EPSG:3857` meters.
#[must_use]
pub fn tile_extent(z: u8) -> f64 {
    EARTH_CIRCUMFERENCE / f64::from(1_u32 << z.min(MAX_ZOOM))
}

/// The ground resolution (meters per pixel) of a `tile_size`-pixel tile at zoom `z`.
#[must_use]
pub fn resolution(z: u8, tile_size: u32) -> f64 {
    tile_extent(z) / f64::from(tile_size)
}

/// Converts an `EPSG:3857` coordinate to `[longitude, latitude]` in degrees.
#[must_use]
pub fn mercator_to_lnglat(x: f64, y: f64) -> [f64; 2] {
    let lng = x / HALF_EARTH * 180.0;
    let lat = (y / HALF_EARTH * PI).sinh().atan().to_degrees();
    [lng, lat]
}

/// The largest latitude Web Mercator can show, in degrees: the north edge of tile `0/0/0`.
pub const MAX_LATITUDE: f64 = 85.051_128_779_806_59;

/// Converts `[longitude, latitude]` in degrees to an `EPSG:3857` coordinate.
/// Latitudes beyond [`MAX_LATITUDE`] are clamped to it.
#[must_use]
pub fn lnglat_to_mercator(lng: f64, lat: f64) -> [f64; 2] {
    let x = lng / 180.0 * HALF_EARTH;
    let lat = lat.clamp(-MAX_LATITUDE, MAX_LATITUDE).to_radians();
    let y = lat.tan().asinh() / PI * HALF_EARTH;
    [x, y]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_validate() {
        assert_eq!(
            "16/32748/21794".parse::<TileCoord>().unwrap(),
            TileCoord::new(16, 32748, 21794).unwrap()
        );
        "1/2/0".parse::<TileCoord>().unwrap_err();
        "1/0".parse::<TileCoord>().unwrap_err();
        TileCoord::new(31, 0, 0).unwrap_err();
    }

    #[test]
    fn bounds() {
        let [min_x, min_y, max_x, max_y] = TileCoord::new(16, 32748, 21794).unwrap().mercator_bounds();
        assert!((min_x - -12_229.924_525_629_729).abs() < 1e-6);
        assert!((max_y - 6_710_559.587_212_194).abs() < 1e-6);
        assert!((max_x - min_x - 611.496_226_281_41).abs() < 1e-6);
        assert!((max_y - min_y - 611.496_226_281_41).abs() < 1e-6);
        let [lng, lat] = mercator_to_lnglat(HALF_EARTH, HALF_EARTH);
        assert!((lng - 180.0).abs() < 1e-9);
        assert!((lat - MAX_LATITUDE).abs() < 1e-9);
        let [x, y] = lnglat_to_mercator(lng, 90.0);
        assert!((x - HALF_EARTH).abs() < 1e-6);
        assert!((y - HALF_EARTH).abs() < 1e-6);
    }
}
