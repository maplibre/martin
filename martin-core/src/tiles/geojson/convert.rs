use geojson::{Geometry, Value};
use geos::Geom as _;
use geozero::ToGeo as _;
use geozero::error::GeozeroError;
use geozero::geos::GeosWriter;

use crate::tiles::geojson::process::process_geojson_geom_n;

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

pub(crate) fn geojson_to_geos_writer(
    geom: &Geometry,
    idx: usize,
) -> Result<GeosWriter, GeozeroError> {
    let mut geos_writer = GeosWriter::new();
    process_geojson_geom_n(geom, idx, &mut geos_writer)?;
    Ok(geos_writer)
}

pub(crate) fn geos_to_geojson(geos_geom: &geos::Geometry) -> Result<Value, GeozeroError> {
    let geo_types_geom = geos_geom.to_geo()?;
    Ok(Value::from(&geo_types_geom))
}

pub(crate) fn convert_validate_simplify_geom(
    mut geom: Geometry,
    idx: usize,
) -> Result<Geometry, GeozeroError> {
    // convert to GEOS geometry
    let geos_writer = geojson_to_geos_writer(&geom, idx)?;

    // validate and simplify geometry
    if geos_writer.geometry().is_valid() {
        let geos_geom_simplified = geos_writer.geometry().simplify(0.0)?;
        let value = geos_to_geojson(&geos_geom_simplified)?;
        geom.value = value;
        return Ok(geom);
    }

    Err(GeozeroError::Geometry("Invalid geometry".into()))
}
