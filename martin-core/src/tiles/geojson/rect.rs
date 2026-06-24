use core::f64;

use geojson::{Geometry, Value};
use i_overlay::core::fill_rule::FillRule;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::clip::FloatClip as _;
use i_overlay::float::single::SingleFloatOverlay as _;
use i_overlay::string::clip::ClipRule;
use martin_tile_utils::tile_bbox;

use crate::tiles::geojson::convert::{
    convert_validate_simplify_geom_geo, line_string_to_shape_path, multi_line_string_from_paths,
    multi_line_string_to_shape_paths, multi_polygon_from_shapes, multi_polygon_to_shape_paths,
    polygon_to_shape_paths,
};
use crate::tiles::geojson::process::tile_length_from_zoom;

pub(crate) const BUFFER_SIZE: u32 = 256;
pub(crate) const EXTENT: u32 = 4096;

#[derive(Debug, Clone)]
pub(crate) struct Rect {
    pub(crate) min_x: f64,
    pub(crate) min_y: f64,
    pub(crate) max_x: f64,
    pub(crate) max_y: f64,
}

impl Default for Rect {
    fn default() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }
}

impl Rect {
    /// Test if point is inside rectangle
    fn inside(&self, point: &[f64]) -> bool {
        // Point has to have at least two coordinates
        let (x, y) = (point[0], point[1]);
        x >= self.min_x && x <= self.max_x && y >= self.min_y && y <= self.max_y
    }

    /// Extend rectangle by point
    pub(crate) fn extend(&mut self, point: &[f64]) {
        let (x, y) = (point[0], point[1]);
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }

    /// Extend with bounding box
    pub(crate) fn extend_by_bbox(&mut self, bbox: &[f64]) {
        // min_x and min_y
        let (x, y) = (bbox[0], bbox[1]);
        self.extend(&[x, y]);

        // max_x and max_y
        let (x, y) = (bbox[2], bbox[3]);
        self.extend(&[x, y]);
    }

    /// Returns the rectangle if it was extended by at least one point.
    /// A default (un-extended) rectangle keeps its infinite corners and yields `None`.
    pub(crate) fn into_finite(self) -> Option<Self> {
        self.min_x.is_finite().then_some(self)
    }

    pub(crate) fn from_xyz(x: u32, y: u32, zoom: u8) -> Self {
        let tile_length = tile_length_from_zoom(zoom);
        let [min_x, min_y, max_x, max_y] = tile_bbox(x, y, tile_length);
        Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    pub(crate) fn from_position(position: &[f64]) -> Self {
        let (x, y) = (position[0], position[1]);
        Self {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        }
    }

    pub(crate) fn from_positions(positions: &[Vec<f64>]) -> Self {
        let mut rect = Rect::default();
        for p in positions {
            rect.extend(p);
        }
        rect
    }

    pub(crate) fn from_linestrings(linestrings: &[Vec<Vec<f64>>]) -> Self {
        let mut rect = Rect::default();

        for l in linestrings {
            for p in l {
                rect.extend(p);
            }
        }
        rect
    }

    pub(crate) fn from_polygons(polygons: &[Vec<Vec<Vec<f64>>>]) -> Self {
        let mut rect = Rect::default();
        for polygon in polygons {
            if let Some(rings) = polygon.first() {
                for point in rings {
                    rect.extend(point);
                }
            }
        }
        rect
    }

    fn rings(&self) -> Vec<Vec<[f64; 2]>> {
        let mut rings = vec![];
        let ring = vec![
            [self.min_x, self.max_y],
            [self.min_x, self.min_y],
            [self.max_x, self.min_y],
            [self.max_x, self.max_y],
        ];

        rings.push(ring);
        rings
    }

    #[expect(clippy::too_many_lines)]
    pub(crate) fn clip_transform_validate_geometry(
        &self,
        mut geom: Geometry,
        idx: usize,
    ) -> Option<Geometry> {
        match geom.value {
            Value::Point(p) => {
                if self.inside(&p) {
                    // transform to tile coordinate system
                    let transformed_p = self.transform_to_tile_coordinates(&p).to_vec();
                    geom.value = Value::Point(transformed_p);
                    Some(geom)
                } else {
                    None
                }
            }
            Value::MultiPoint(ps) => {
                let filtered_ps: Vec<Vec<f64>> =
                    ps.into_iter().filter(|p| self.inside(p)).collect();

                if filtered_ps.is_empty() {
                    return None;
                }

                // transform to tile coordinate system
                let transformed_ps = filtered_ps
                    .iter()
                    .map(|p| {
                        let coords = self.transform_to_tile_coordinates(p);
                        coords.to_vec()
                    })
                    .collect();

                geom.value = Value::MultiPoint(transformed_ps);
                Some(geom)
            }
            Value::LineString(ls) => {
                let string_line = line_string_to_shape_path(ls);
                let clip = self.rings();
                let clipped_ls = string_line.clip_by(
                    &clip,
                    FillRule::NonZero,
                    ClipRule {
                        invert: false,
                        boundary_included: false,
                    },
                );

                if clipped_ls.is_empty() {
                    return None;
                }

                // transform to tile coordinate system
                let transformed_ls = clipped_ls
                    .iter()
                    .map(|vec| {
                        vec.iter()
                            .map(|p| self.transform_to_tile_coordinates(p))
                            .collect()
                    })
                    .collect();

                geom.value = Value::MultiLineString(multi_line_string_from_paths(transformed_ls));

                // validate and simplify (remove duplicate points)
                convert_validate_simplify_geom_geo(geom, idx).ok()
            }
            Value::MultiLineString(ls) => {
                let string_line = multi_line_string_to_shape_paths(ls);
                let clip = self.rings();
                let clipped_ls = string_line.clip_by(
                    &clip,
                    FillRule::NonZero,
                    ClipRule {
                        invert: false,
                        boundary_included: false,
                    },
                );

                if clipped_ls.is_empty() {
                    return None;
                }

                // transform to tile coordinate system
                let transformed_ls = clipped_ls
                    .iter()
                    .map(|vec| {
                        vec.iter()
                            .map(|p| self.transform_to_tile_coordinates(p))
                            .collect()
                    })
                    .collect();
                geom.value = Value::MultiLineString(multi_line_string_from_paths(transformed_ls));

                // validate and simplify (remove duplicate points)
                convert_validate_simplify_geom_geo(geom, idx).ok()
            }
            Value::Polygon(polygon) => {
                let subject = self.rings();
                let clip = polygon_to_shape_paths(polygon);
                let polygons = subject.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);

                if polygons.is_empty() {
                    return None;
                }

                // transform to tile coordinate system
                let transformed_polygons = polygons
                    .iter()
                    .map(|polygon| {
                        polygon
                            .iter()
                            .map(|ring| {
                                ring.iter()
                                    .map(|p| self.transform_to_tile_coordinates(p))
                                    .collect()
                            })
                            .collect()
                    })
                    .collect();

                geom.value = multi_polygon_from_shapes(transformed_polygons);

                // validate and simplify (remove duplicate points)
                convert_validate_simplify_geom_geo(geom, idx).ok()
            }
            Value::MultiPolygon(polygons) => {
                let subject = self.rings();
                let clip = multi_polygon_to_shape_paths(polygons);
                let polygons = subject.overlay(&clip, OverlayRule::Intersect, FillRule::EvenOdd);

                if polygons.is_empty() {
                    return None;
                }

                // transform to tile coordinate system
                let transformed_polygons = polygons
                    .iter()
                    .map(|polygon| {
                        polygon
                            .iter()
                            .map(|ring| {
                                ring.iter()
                                    .map(|p| self.transform_to_tile_coordinates(p))
                                    .collect()
                            })
                            .collect()
                    })
                    .collect();

                geom.value = multi_polygon_from_shapes(transformed_polygons);

                // validate and simplify (remove duplicate points)
                convert_validate_simplify_geom_geo(geom, idx).ok()
            }
            Value::GeometryCollection(gs) => {
                let mut geometries = vec![];
                for (idx, g) in gs.into_iter().enumerate() {
                    if let Some(value) = self.clip_transform_validate_geometry(g, idx) {
                        geometries.push(value);
                    }
                }

                if geometries.is_empty() {
                    return None;
                }

                geom.value = Value::GeometryCollection(geometries);
                Some(geom)
            }
        }
    }

    /// Transform from EPSG:3857 to local MVT tile coordinates
    fn transform_to_tile_coordinates(&self, point: &[f64]) -> [f64; 2] {
        let x = point[0];
        let y = point[1];

        // Take buffer into account and convert to original tile boundaries
        let buffer = f64::from(BUFFER_SIZE) / f64::from(EXTENT);

        let max_x = ((1.0 + buffer) * self.max_x + buffer * self.min_x) / (1.0 + 2.0 * buffer);
        let min_x = (self.min_x + max_x * buffer) / (1.0 + buffer);

        let max_y = ((1.0 + buffer) * self.max_y + buffer * self.min_y) / (1.0 + 2.0 * buffer);
        let min_y = (self.min_y + max_y * buffer) / (1.0 + buffer);

        let x_multiplier = f64::from(EXTENT) / (max_x - min_x);
        let y_multiplier = f64::from(EXTENT) / (max_y - min_y);

        let x = ((x - min_x) * x_multiplier).floor();
        let y = f64::from(EXTENT) - ((y - min_y) * y_multiplier).floor();

        [x, y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transform_to_tile_coordinates() {
        let point = [1_962_772.0, 6_300_000.0];
        let mut rect = Rect::from_xyz(70, 43, 7);
        let buffer = f64::from(BUFFER_SIZE) / f64::from(EXTENT);
        let buffer_x = (rect.max_x - rect.min_x) * buffer;
        let buffer_y = (rect.max_y - rect.min_y) * buffer;
        rect.min_x -= buffer_x;
        rect.min_y -= buffer_y;
        rect.max_x += buffer_x;
        rect.max_y += buffer_y;
        let transformed_point = rect.transform_to_tile_coordinates(&point);
        // `transform_to_tile_coordinates` floors to integer tile coordinates, so the
        // result is exact.
        assert_eq!(transformed_point, [1102.0, 3596.0]);
    }
}
