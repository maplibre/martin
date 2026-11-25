use core::f64;
use std::num::NonZeroU32;

use geo::bool_ops::FillRule;
use geo::orient::Direction;
use geo::{Area as _, BooleanOps as _, MapCoords as _, Orient as _, Validation as _};
use geo_types::{Coord, Geometry, GeometryCollection, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon};
use martin_tile_utils::tile_bbox;

use crate::tiles::geojson::convert::validate_and_simplify;
use crate::tiles::geojson::process::tile_length_from_zoom;

/// A single tile in Web Mercator space, carrying the MVT resolution it is rendered at.
#[derive(Debug, Clone)]
pub struct Rect {
    pub(crate) min_x: f64,
    pub(crate) min_y: f64,
    pub(crate) max_x: f64,
    pub(crate) max_y: f64,
    /// Side length of the MVT tile coordinate grid this tile is encoded into.
    extent: NonZeroU32,
    /// Clip margin in tile units kept around the tile edge, expressed as a fraction of `extent`.
    buffer: u32,
}

impl Rect {
    /// Test if point is inside rectangle
    fn inside(&self, point: &[f64]) -> bool {
        // Point has to have at least two coordinates
        let (x, y) = (point[0], point[1]);
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    pub(crate) fn from_xyz(x: u32, y: u32, zoom: u8, extent: NonZeroU32, buffer: u32) -> Self {
        let tile_length = tile_length_from_zoom(zoom);
        let [min_x, min_y, max_x, max_y] = tile_bbox(x, y, tile_length);
        Self {
            min_x,
            min_y,
            max_x,
            max_y,
            extent,
            buffer,
        }
    }

    /// The clip margin as a fraction of the tile width, e.g. `64 / 4096`.
    fn buffer_fraction(&self) -> f64 {
        f64::from(self.buffer) / f64::from(self.extent.get())
    }

    /// Grow the tile outward by the buffer fraction so geometry just outside the tile is still
    /// fetched and clipped into the buffer margin.
    pub(crate) fn add_buffer(&mut self) {
        let fraction = self.buffer_fraction();
        let buffer_x = (self.max_x - self.min_x) * fraction;
        let buffer_y = (self.max_y - self.min_y) * fraction;
        self.min_x -= buffer_x;
        self.min_y -= buffer_y;
        self.max_x += buffer_x;
        self.max_y += buffer_y;
    }

    /// The (buffered) tile rectangle as a clip polygon in Web Mercator coordinates.
    fn clip_polygon(&self) -> Polygon<f64> {
        geo_types::Rect::new(
            Coord {
                x: self.min_x,
                y: self.min_y,
            },
            Coord {
                x: self.max_x,
                y: self.max_y,
            },
        )
        .to_polygon()
    }

    /// Clip a 1-D geometry to the tile, snap to the integer grid, and validate; `None` if nothing remains.
    fn clip_lines(&self, lines: &MultiLineString<f64>) -> Option<Geometry<f64>> {
        let clipped = self.clip_polygon().clip(lines, false);
        if clipped.0.is_empty() {
            return None;
        }
        let tile_space = clipped.map_coords(|c| self.to_tile_coord(c));
        validate_and_simplify(tile_space.into())
    }

    /// Intersect a 2-D geometry with the tile, snap to the integer grid, orient for MVT, and validate; `None` if nothing remains.
    fn clip_area(&self, area: &impl geo::BooleanOps<Scalar = f64>) -> Option<Geometry<f64>> {
        let clipped = self.clip_polygon().intersection_with_fill_rule(area, FillRule::EvenOdd);
        if clipped.0.is_empty() {
            return None;
        }
        let snapped = clipped.map_coords(|c| self.to_tile_coord(c));
        // The integer snap can pinch a polygon into a self-touch; re-resolve it through the overlay
        // engine so the topology is repaired rather than failing validation and dropping the feature.
        let resolved = if snapped.is_valid() {
            snapped
        } else {
            resolve_snapped(&snapped)
        };
        if resolved.0.is_empty() {
            // The snap collapsed the polygon below tile resolution; drop it rather than emit an
            // empty geometry.
            return None;
        }
        // The snap flips y, reversing ring orientation; re-orient so exterior rings are
        // counter-clockwise (MVT's required winding once y points down in tile space).
        let tile_space = resolved.orient(Direction::Default);
        validate_and_simplify(tile_space.into())
    }

    /// Clip a Web Mercator geometry to this (buffered) tile, snap it to the integer MVT grid, and
    /// validate; `None` when nothing of the geometry remains inside the tile.
    pub(crate) fn clip_transform_validate_geometry(&self, geom: Geometry<f64>) -> Option<Geometry<f64>> {
        match geom {
            Geometry::Point(p) => self
                .inside(&[p.x(), p.y()])
                .then(|| Geometry::Point(self.to_tile_coord(p.0).into())),
            Geometry::MultiPoint(ps) => {
                let kept: Vec<Point<f64>> = ps
                    .into_iter()
                    .filter(|p| self.inside(&[p.x(), p.y()]))
                    .map(|p| self.to_tile_coord(p.0).into())
                    .collect();
                (!kept.is_empty()).then_some(Geometry::MultiPoint(MultiPoint(kept)))
            }
            Geometry::LineString(ls) => self.clip_lines(&MultiLineString(vec![ls])),
            Geometry::MultiLineString(mls) => self.clip_lines(&mls),
            Geometry::Polygon(polygon) => self.clip_area(&polygon),
            Geometry::MultiPolygon(polygons) => self.clip_area(&polygons),
            Geometry::GeometryCollection(gs) => {
                let kept: Vec<Geometry<f64>> = gs
                    .into_iter()
                    .filter_map(|g| self.clip_transform_validate_geometry(g))
                    .collect();
                (!kept.is_empty()).then_some(Geometry::GeometryCollection(GeometryCollection(kept)))
            }
            // GeoJSON never parses into these geometry variants.
            Geometry::Line(_) | Geometry::Rect(_) | Geometry::Triangle(_) => None,
        }
    }

    /// Transform a Web Mercator coordinate into the integer-snapped MVT tile grid.
    fn to_tile_coord(&self, c: Coord<f64>) -> Coord<f64> {
        let [x, y] = self.transform_to_tile_coordinates(&[c.x, c.y]);
        Coord { x, y }
    }

    /// Transform from EPSG:3857 to local MVT tile coordinates
    fn transform_to_tile_coordinates(&self, point: &[f64]) -> [f64; 2] {
        let x = point[0];
        let y = point[1];

        // Take buffer into account and convert to original tile boundaries
        let buffer = self.buffer_fraction();
        let extent = f64::from(self.extent.get());

        let max_x = buffer.mul_add(self.min_x, (1.0 + buffer) * self.max_x) / 2.0f64.mul_add(buffer, 1.0);
        let min_x = max_x.mul_add(buffer, self.min_x) / (1.0 + buffer);

        let max_y = buffer.mul_add(self.min_y, (1.0 + buffer) * self.max_y) / 2.0f64.mul_add(buffer, 1.0);
        let min_y = max_y.mul_add(buffer, self.min_y) / (1.0 + buffer);

        let x_multiplier = extent / (max_x - min_x);
        let y_multiplier = extent / (max_y - min_y);

        let x = ((x - min_x) * x_multiplier).floor();
        let y = extent - ((y - min_y) * y_multiplier).floor();

        [x, y]
    }
}

/// Repair a polygon the integer snap made invalid, filling by the sign of its area.
///
/// `unary_union` infers the fill rule from the turn at one vertex, which a folded spike or loop can hide or reverse,
/// and then keeps only what lies outside the polygon.
fn resolve_snapped(snapped: &MultiPolygon<f64>) -> MultiPolygon<f64> {
    let fill_rule = if snapped.signed_area() > 0.0 {
        FillRule::Positive
    } else {
        FillRule::Negative
    };
    snapped.union_with_fill_rule(&MultiPolygon::empty(), fill_rule)
}

#[cfg(test)]
mod tests {
    use geo_types::LineString;
    use martin_tile_utils::wgs84_to_webmercator;
    use rstest::rstest;

    use super::*;

    fn mercator_ring(lng_lat: &[(f64, f64)]) -> LineString<f64> {
        lng_lat
            .iter()
            .map(|&(lng, lat)| {
                let (x, y) = wgs84_to_webmercator(lng, lat);
                Coord { x, y }
            })
            .collect()
    }

    /// A 10° square with a 2° hole. With `spike`, its west edge carries a spike thin enough for the integer snap to
    /// fold it onto one column at low zoom, leaving the exterior without a definite winding while the hole keeps one.
    fn square_with_hole(spike: bool) -> Polygon<f64> {
        let mut exterior = vec![(0.01, 0.0), (10.0, 0.0), (10.0, 10.0), (0.01, 10.0)];
        if spike {
            exterior.extend([(0.01, 5.032), (-0.01, 5.031), (0.01, 5.03)]);
        }
        exterior.push((0.01, 0.0));
        let hole = [(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0), (2.0, 2.0)];
        Polygon::new(mercator_ring(&exterior), vec![mercator_ring(&hole)])
    }

    fn clip_to_tile(polygon: Polygon<f64>, zoom: u8, x: u32, y: u32) -> Option<Geometry<f64>> {
        let extent = NonZeroU32::new(4096).expect("4096 is non-zero");
        let mut rect = Rect::from_xyz(x, y, zoom, extent, 64);
        rect.add_buffer();
        rect.clip_transform_validate_geometry(polygon.into())
    }

    #[rstest]
    #[case::z0(0, 0, 0)]
    #[case::z1(1, 1, 0)]
    #[case::z2(2, 2, 1)]
    #[case::z3(3, 4, 3)]
    fn clip_keeps_a_polygon_with_a_hole_when_the_snap_folds_a_spike(#[case] zoom: u8, #[case] x: u32, #[case] y: u32) {
        let Some(Geometry::MultiPolygon(clipped)) = clip_to_tile(square_with_hole(true), zoom, x, y) else {
            panic!("the polygon must survive the clip at {zoom}/{x}/{y}");
        };
        assert_eq!(clipped.0.len(), 1);
        assert_eq!(clipped.0[0].interiors().len(), 1, "the hole is kept");

        // The spike is thinner than a pixel, so the repaired polygon covers what the spike-free one does.
        let Some(Geometry::MultiPolygon(reference)) = clip_to_tile(square_with_hole(false), zoom, x, y) else {
            panic!("the spike-free polygon must survive the clip at {zoom}/{x}/{y}");
        };
        approx::assert_relative_eq!(clipped.unsigned_area(), reference.unsigned_area(), max_relative = 0.01);
    }

    #[test]
    fn transform_to_tile_coordinates() {
        let point = [1_962_772.0, 6_300_000.0];
        let extent = NonZeroU32::new(4096).expect("4096 is non-zero");
        let mut rect = Rect::from_xyz(70, 43, 7, extent, 256);
        rect.add_buffer();
        let transformed_point = rect.transform_to_tile_coordinates(&point);
        // `transform_to_tile_coordinates` floors to integer tile coordinates, so the
        // result is exact.
        approx::assert_relative_eq!(transformed_point[..], [1102.0, 3596.0][..]);
    }
}
