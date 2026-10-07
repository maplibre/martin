//! Source coordinates to Web Mercator unit coordinates: `x` and `y` in `[0, 1]`, `y` pointing south,
//! so `unit * 2^z * extent` is a zoom's tile grid with tiles' own y-down orientation.

use std::f64::consts::{FRAC_PI_4, PI};

use geo_types::Coord;

/// Latitude where Web Mercator's square world ends.
pub const MAX_LATITUDE: f64 = 85.051_128_779_806_59;
/// Half the Web Mercator world width in meters.
const HALF_WORLD: f64 = 20_037_508.342_789_244;

/// WGS84 degrees, latitude clamped to the Mercator square. Longitudes beyond ±180 are kept: the
/// renderer moves or wraps such features rather than distorting them.
pub fn from_lonlat(coords: &mut [Coord<f64>]) {
    for c in coords {
        let lat = c.y.clamp(-MAX_LATITUDE, MAX_LATITUDE).to_radians();
        c.x = (c.x + 180.0) / 360.0;
        c.y = 0.5 - (FRAC_PI_4 + lat / 2.0).tan().ln() / (2.0 * PI);
    }
}

/// EPSG:3857 meters, `y` clamped to the square.
pub fn from_mercator(coords: &mut [Coord<f64>]) {
    for c in coords {
        c.x = (c.x + HALF_WORLD) / (2.0 * HALF_WORLD);
        c.y = ((HALF_WORLD - c.y) / (2.0 * HALF_WORLD)).clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use approx::assert_abs_diff_eq;

    use super::*;

    #[test]
    fn corners_and_center() {
        let mut lonlat = [
            Coord {
                x: -180.0,
                y: MAX_LATITUDE,
            },
            Coord { x: 0.0, y: 0.0 },
            Coord { x: 180.0, y: -90.0 },
            Coord { x: 190.0, y: 0.0 },
        ];
        from_lonlat(&mut lonlat);
        let expected = [
            (0.0, 0.0),
            (0.5, 0.5),
            (1.0, 1.0),
            (190.0 / 360.0 + 0.5, 0.5),
        ];
        for (c, (x, y)) in lonlat.iter().zip(expected) {
            assert_abs_diff_eq!(c.x, x, epsilon = 1e-12);
            assert_abs_diff_eq!(c.y, y, epsilon = 1e-12);
        }

        let mut meters = [
            Coord {
                x: -HALF_WORLD,
                y: HALF_WORLD * 2.0,
            },
            Coord { x: 0.0, y: 0.0 },
        ];
        from_mercator(&mut meters);
        assert_abs_diff_eq!(meters[0].x, 0.0);
        assert_abs_diff_eq!(meters[0].y, 0.0, epsilon = 1e-12);
        assert_abs_diff_eq!(meters[1].x, 0.5);
        assert_abs_diff_eq!(meters[1].y, 0.5);
    }

    #[test]
    fn matches_mercator_meters() {
        // Berlin: 13.405 E, 52.52 N is (1_492_237, 6_894_699) in EPSG:3857.
        let mut a = [Coord {
            x: 13.405,
            y: 52.52,
        }];
        let mut b = [Coord {
            x: 1_492_237.4,
            y: 6_894_699.8,
        }];
        from_lonlat(&mut a);
        from_mercator(&mut b);
        assert_abs_diff_eq!(a[0].x, b[0].x, epsilon = 1e-7);
        assert_abs_diff_eq!(a[0].y, b[0].y, epsilon = 1e-7);
    }
}
