//! Web Mercator projection between lon/lat and image pixel coordinates.

use geo_types::Coord;
use martin_tile_utils::{EARTH_CIRCUMFERENCE, wgs84_to_webmercator};

/// Pixel position of `coord` in a `img_width × img_height` image
/// centered on `center` at the given `zoom`.
///
/// Pixel `(0, 0)` is the top-left of the image; the y-axis grows downward
/// while Web Mercator's y grows northward, so y is flipped here.
#[must_use]
pub(super) fn geo_to_pixel(
    coord: Coord,
    zoom: f64,
    img_width: u32,
    img_height: u32,
    center: Coord,
) -> (f64, f64) {
    let scale = 256.0 * 2_f64.powf(zoom);
    let (px, py) = wgs84_to_pixel(coord, scale);
    let (cx, cy) = wgs84_to_pixel(center, scale);
    (
        px - cx + f64::from(img_width) / 2.0,
        py - cy + f64::from(img_height) / 2.0,
    )
}

/// `(lon, lat)` → pixel coordinates at `scale = 256 * 2^zoom`, in the
/// slippy-map pixel frame: x grows east, y grows south.
fn wgs84_to_pixel(coord: Coord, scale: f64) -> (f64, f64) {
    let (merc_x, merc_y) = wgs84_to_webmercator(coord.x, coord.y);
    (
        (merc_x / EARTH_CIRCUMFERENCE + 0.5) * scale,
        (-merc_y / EARTH_CIRCUMFERENCE + 0.5) * scale,
    )
}

#[cfg(test)]
mod tests {
    use approx::assert_relative_eq;
    use geo_types::{Point, wkt};
    use rstest::rstest;

    use super::*;

    /// The camera's own coordinate always lands on the image center.
    #[rstest]
    #[case::origin_zoom_0(wkt! { POINT(0.0 0.0) }, 0.0, 256, 256)]
    #[case::berlin_zoom_3(wkt! { POINT(13.4 52.5) }, 3.0, 800, 600)]
    #[case::antimeridian_zoom_2(wkt! { POINT(180.0 0.0) }, 2.0, 400, 400)]
    fn center_maps_to_image_center(
        #[case] point: Point,
        #[case] zoom: f64,
        #[case] w: u32,
        #[case] h: u32,
    ) {
        let c = point.into();
        let (px, py) = geo_to_pixel(c, zoom, w, h, c);
        assert_relative_eq!(px, f64::from(w) / 2.0, epsilon = 1e-9);
        assert_relative_eq!(py, f64::from(h) / 2.0, epsilon = 1e-9);
    }

    /// East of camera is +x, west is -x; north is -y, south is +y -
    /// image y grows south while Mercator y grows north.
    #[test]
    fn axes_point_east_and_south() {
        let center = wkt! { POINT(0.0 0.0) }.into();
        let (east_px, _) = geo_to_pixel(wkt! { POINT(10.0 0.0) }.into(), 4.0, 0, 0, center);
        let (west_px, _) = geo_to_pixel(wkt! { POINT(-10.0 0.0) }.into(), 4.0, 0, 0, center);
        assert!(east_px > 0.0 && west_px < 0.0);

        let (_, north_py) = geo_to_pixel(wkt! { POINT(0.0 10.0) }.into(), 4.0, 0, 0, center);
        let (_, south_py) = geo_to_pixel(wkt! { POINT(0.0 -10.0) }.into(), 4.0, 0, 0, center);
        assert!(north_py < 0.0 && south_py > 0.0);
    }

    /// Each extra zoom level doubles the pixel offset from the camera
    /// (scale = 256 * 2^zoom).
    #[test]
    fn zoom_doubles_pixel_offset() {
        let point = wkt! { POINT(45.0 30.0) }.into();
        let center = wkt! { POINT(0.0 0.0) }.into();
        let (x0, y0) = geo_to_pixel(point, 3.0, 0, 0, center);
        let (x1, y1) = geo_to_pixel(point, 4.0, 0, 0, center);
        assert_relative_eq!(x1, 2.0 * x0, epsilon = 1e-9);
        assert_relative_eq!(y1, 2.0 * y0, epsilon = 1e-9);
    }

    /// At zoom 0 the world is 256 px wide, so the antimeridian sits 128 px
    /// east of the prime-meridian camera. Locks down the absolute scale.
    #[test]
    fn antimeridian_at_zoom_0_is_128px_east() {
        let center = wkt! { POINT(0.0 0.0) }.into();
        let (px, _) = geo_to_pixel(wkt! { POINT(180.0 0.0) }.into(), 0.0, 0, 0, center);
        assert_relative_eq!(px, 128.0, epsilon = 1e-9);
    }
}
