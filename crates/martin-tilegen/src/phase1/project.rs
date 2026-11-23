//! Source coordinates to Web Mercator unit coordinates: `x` and `y` in `[0, 1]`, `y` pointing south,
//! so `unit * 2^z * extent` is a zoom's tile grid with tiles' own y-down orientation.

use geo_types::Coord;
use martin_tile_utils::{EARTH_CIRCUMFERENCE, wgs84_to_webmercator};

/// Latitude where Web Mercator's square world ends.
pub const MAX_LATITUDE: f64 = 85.051_128_779_806_59;
/// Half the Web Mercator world width in meters.
const HALF_WORLD: f64 = EARTH_CIRCUMFERENCE / 2.0;

/// WGS84 degrees, latitude clamped to the Mercator square. Longitudes beyond ±180 are kept: the
/// renderer moves or wraps such features rather than distorting them.
#[must_use]
pub fn from_lonlat(c: Coord<f64>) -> Coord<f64> {
    let (x, y) = wgs84_to_webmercator(c.x, c.y.clamp(-MAX_LATITUDE, MAX_LATITUDE));
    from_mercator(Coord { x, y })
}

/// EPSG:3857 meters, `y` clamped to the square.
#[must_use]
pub fn from_mercator(c: Coord<f64>) -> Coord<f64> {
    Coord {
        x: (c.x + HALF_WORLD) / (2.0 * HALF_WORLD),
        y: ((HALF_WORLD - c.y) / (2.0 * HALF_WORLD)).clamp(0.0, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    #[test]
    fn corners_and_center() {
        let lonlat = [
            Coord {
                x: -180.0,
                y: MAX_LATITUDE,
            },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 180.0, y: -90.0 },
            Coord { x: 190.0, y: 0.0 },
        ];
        let expected = [(0.0, 0.0), (0.5, 0.5), (1.0, 1.0), (190.0 / 360.0 + 0.5, 0.5)];
        for (c, (x, y)) in lonlat.into_iter().map(from_lonlat).zip(expected) {
            assert_abs_diff_eq!(c.x, x, epsilon = 1e-12);
            assert_abs_diff_eq!(c.y, y, epsilon = 1e-12);
        }

        let corner = from_mercator(Coord {
            x: -HALF_WORLD,
            y: HALF_WORLD * 2.0,
        });
        assert_abs_diff_eq!(corner.x, 0.0);
        assert_abs_diff_eq!(corner.y, 0.0, epsilon = 1e-12);
        let center = from_mercator(Coord { x: 0.0, y: 0.0 });
        assert_abs_diff_eq!(center.x, 0.5);
        assert_abs_diff_eq!(center.y, 0.5);
    }

    #[test]
    fn matches_mercator_meters() {
        // Berlin: 13.405 E, 52.52 N is (1_492_237, 6_894_699) in EPSG:3857.
        let a = from_lonlat(Coord { x: 13.405, y: 52.52 });
        let b = from_mercator(Coord {
            x: 1_492_237.4,
            y: 6_894_699.8,
        });
        assert_abs_diff_eq!(a.x, b.x, epsilon = 1e-7);
        assert_abs_diff_eq!(a.y, b.y, epsilon = 1e-7);
    }
}
