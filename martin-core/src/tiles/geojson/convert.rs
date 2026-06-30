use geo::{Simplify as _, Validation as _};

const EPS: f64 = 1e-9;

/// Drop duplicate/collinear points within `EPS`; points and multipoints are returned unchanged.
fn simplify_geo(geom: geo_types::Geometry<f64>) -> geo_types::Geometry<f64> {
    match geom {
        point @ geo::Geometry::Point(_) => point,
        points @ geo::Geometry::MultiPoint(_) => points,
        geo::Geometry::LineString(linestring) => {
            geo::Geometry::LineString(linestring.simplify(EPS))
        }
        geo::Geometry::MultiLineString(multi_linestring) => {
            geo::Geometry::MultiLineString(multi_linestring.simplify(EPS))
        }
        geo::Geometry::Polygon(polygon) => geo::Geometry::Polygon(polygon.simplify(EPS)),
        geo::Geometry::MultiPolygon(multi_polygon) => {
            geo::Geometry::MultiPolygon(multi_polygon.simplify(EPS))
        }
        rest => rest,
    }
}

/// Validate a tile-space geometry and drop duplicate points.
/// Geometry that the integer snap pinched into an invalid shape (e.g. a self-touching polygon) yields `None`.
pub(crate) fn validate_and_simplify(
    geom: geo_types::Geometry<f64>,
) -> Option<geo_types::Geometry<f64>> {
    geom.is_valid().then(|| simplify_geo(geom))
}
