use geojson::Value;

pub fn line_string_from_path(path: Vec<[f64; 2]>) -> Vec<Vec<f64>> {
    path.into_iter()
        .map(|coord| vec![coord[0], coord[1]])
        .collect()
}

pub fn multi_line_string_from_paths(paths: Vec<Vec<[f64; 2]>>) -> Vec<Vec<Vec<f64>>> {
    paths
        .into_iter()
        .map(|p| line_string_from_path(p))
        .collect()
}

pub fn line_string_to_shape_path(line_string: Vec<Vec<f64>>) -> Vec<[f64; 2]> {
    line_string.iter().map(|v| [v[0], v[1]]).collect()
}

pub fn multi_line_string_to_shape_path(line_strings: Vec<Vec<Vec<f64>>>) -> Vec<Vec<[f64; 2]>> {
    line_strings
        .into_iter()
        .map(line_string_to_shape_path)
        .collect()
}

pub fn rings_from_shape(shape: Vec<Vec<[f64; 2]>>) -> Vec<Vec<Vec<f64>>> {
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

pub fn polygon_from_shape(shape: Vec<Vec<[f64; 2]>>) -> Value {
    let rings = rings_from_shape(shape);
    Value::Polygon(rings)
}

pub fn multi_polygon_from_shapes(shapes: Vec<Vec<Vec<[f64; 2]>>>) -> Value {
    let polygons = shapes.into_iter().map(|s| rings_from_shape(s));
    Value::MultiPolygon(polygons.collect())
}

pub fn ring_to_shape_path(line_string: Vec<Vec<f64>>) -> Vec<[f64; 2]> {
    if line_string.is_empty() {
        return vec![];
    }
    // In geo, Polygon rings are explicitly closed LineStrings — their final coordinate is the same as their first coordinate,
    // however in i_overlay, shape paths are implicitly closed, so we skip the last coordinate.
    let coords = &line_string[..line_string.len() - 1];
    coords.iter().map(|p| [p[0], p[1]]).collect()
}

pub fn polygon_to_shape_path(polygon: Vec<Vec<Vec<f64>>>) -> Vec<Vec<[f64; 2]>> {
    polygon
        .into_iter()
        .map(ring_to_shape_path)
        .collect::<Vec<_>>()
}

pub fn multi_polygon_to_shape_path(
    multi_polygon: Vec<Vec<Vec<Vec<f64>>>>,
) -> Vec<Vec<Vec<[f64; 2]>>> {
    multi_polygon
        .into_iter()
        .map(polygon_to_shape_path)
        .collect::<Vec<_>>()
}
