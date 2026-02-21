use geo::{Simplify as _, Validation as _};
use geojson::{Geometry, Value};
use geozero::error::GeozeroError;

pub(crate) fn line_string_from_path(path: Vec<[f64; 2]>) -> Vec<Vec<f64>> {
    path.into_iter()
        .rev()
        .map(|coord| vec![coord[0], coord[1]])
        .collect()
}

pub(crate) fn multi_line_string_from_paths(paths: Vec<Vec<[f64; 2]>>) -> Vec<Vec<Vec<f64>>> {
    paths.into_iter().map(line_string_from_path).collect()
}

pub(crate) fn line_string_to_shape_path(line_string: Vec<Vec<f64>>) -> Vec<[f64; 2]> {
    line_string.into_iter().map(|v| [v[0], v[1]]).collect()
}

pub(crate) fn multi_line_string_to_shape_paths(
    line_strings: Vec<Vec<Vec<f64>>>,
) -> Vec<Vec<[f64; 2]>> {
    line_strings
        .into_iter()
        .map(line_string_to_shape_path)
        .collect()
}

pub(crate) fn rings_from_shape(shape: Vec<Vec<[f64; 2]>>) -> Vec<Vec<Vec<f64>>> {
    shape
        .into_iter()
        .map(|path| {
            let mut line_string = line_string_from_path(path);
            if line_string.first() != line_string.last() {
                line_string.push(vec![line_string[0][0], line_string[0][1]]);
            }
            line_string
        })
        .collect()
}

pub(crate) fn multi_polygon_from_shapes(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Value {
    let polygons = shapes.into_iter().map(rings_from_shape);
    Value::MultiPolygon(polygons.collect())
}

pub(crate) fn ring_to_shape_path(mut line_string: Vec<Vec<f64>>) -> Vec<[f64; 2]> {
    if line_string.is_empty() {
        return vec![];
    }
    // i_overlay does not explicitly close rings - skip last coordinate
    let _ = line_string.pop();
    line_string.into_iter().map(|v| [v[0], v[1]]).collect()
}

pub(crate) fn polygon_to_shape_paths(polygon: Vec<Vec<Vec<f64>>>) -> Vec<Vec<[f64; 2]>> {
    polygon
        .into_iter()
        .map(ring_to_shape_path)
        .collect::<Vec<_>>()
}

pub(crate) fn multi_polygon_to_shape_paths(
    multi_polygon: Vec<Vec<Vec<Vec<f64>>>>,
) -> Vec<Vec<Vec<[f64; 2]>>> {
    multi_polygon
        .into_iter()
        .map(polygon_to_shape_paths)
        .collect::<Vec<_>>()
}

const EPS: f64 = 1e-9;
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

pub(crate) fn convert_validate_simplify_geom_geo(
    mut geom: Geometry,
    _idx: usize,
) -> Result<Geometry, GeozeroError> {
    // convert to geo geometry
    let geo_geom = geo_types::Geometry::<f64>::try_from(geom.value)?;

    // validate and simplify (remove duplicates) geometry
    if geo_geom.is_valid() {
        let geo_geom_simplified = simplify_geo(geo_geom);
        geom.value = Value::from(&geo_geom_simplified);
        return Ok(geom);
    }

    Err(GeozeroError::Geometry("Invalid geometry".into()))
}
