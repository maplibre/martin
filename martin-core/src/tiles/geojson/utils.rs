use std::collections::BTreeMap;

use geozero::{ColumnValue, GeomProcessor, PropertyProcessor, mvt::MvtWriter};

/// Helper functions to convert serde_json numbers into geozero::ColumnValue.
pub fn serde_number_to_geozero(
    number: &serde_json::value::Number,
    prefer_f32: bool,
) -> ColumnValue<'_> {
    if number.is_u64() {
        let number_u64 = number.as_u64().unwrap();
        if u8::MIN as u64 <= number_u64 && number_u64 <= u8::MAX as u64 {
            ColumnValue::UByte(number_u64 as u8)
        } else if u16::MIN as u64 <= number_u64 && number_u64 <= u16::MAX as u64 {
            ColumnValue::UShort(number_u64 as u16)
        } else if u32::MIN as u64 <= number_u64 && number_u64 <= u32::MAX as u64 {
            ColumnValue::UInt(number_u64 as u32)
        } else {
            ColumnValue::ULong(number_u64)
        }
    } else if number.is_i64() {
        let number_i64 = number.as_i64().unwrap();
        if i8::MIN as i64 <= number_i64 && number_i64 <= i8::MAX as i64 {
            ColumnValue::Byte(number_i64 as i8)
        } else if i16::MIN as i64 <= number_i64 && number_i64 <= i16::MAX as i64 {
            ColumnValue::Short(number_i64 as i16)
        } else if i32::MIN as i64 <= number_i64 && number_i64 <= i32::MAX as i64 {
            ColumnValue::Int(number_i64 as i32)
        } else {
            ColumnValue::Long(number_i64)
        }
    } else {
        assert!(number.is_f64());
        let number_f64 = number.as_f64().unwrap();
        if prefer_f32 {
            ColumnValue::Float(number_f64 as f32)
        } else {
            ColumnValue::Double(number_f64)
        }
    }
}

/// Describes the type of property for use in the TileJSON.
fn property_type(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::String(_) => "string",
        serde_json::Value::Number(n) => {
            if n.is_u64() {
                "unsigned integer"
            } else if n.is_i64() {
                "signed integer"
            } else if n.is_f64() {
                "double"
            } else {
                unreachable!()
            }
        }
        serde_json::Value::Bool(_) => "boolean",
        _ => "string",
    }
}

/// Helper to add GeoJSON properties to the MvtWriter.
pub fn write_geojson_properties(
    mvt_writer: &mut MvtWriter,
    idx: usize,
    properties: &geojson::JsonObject,
) {
    for (key, json_value) in properties.iter() {
        let stringified_json: String;
        let value = match json_value {
            serde_json::Value::Bool(bool) => ColumnValue::Bool(*bool),
            serde_json::Value::Number(number) => serde_number_to_geozero(number, false),
            serde_json::Value::String(str) => ColumnValue::String(str),
            json_value => {
                stringified_json = json_value.to_string();
                ColumnValue::Json(&stringified_json)
            }
        };
        let _ = mvt_writer.property(idx, key, &value);
    }
}

/// Helper to add a GeoJSON geometry to the MvtWriter.
pub fn write_geojson_geom(mvt_writer: &mut MvtWriter, geom: &geojson::Value) {
    match geom {
        geojson::Value::Point(point) => {
            let x = point[0];
            let y = point[1];
            let _ = mvt_writer.point_begin(0);
            let _ = mvt_writer.xy(x, y, 0);
            let _ = mvt_writer.point_end(0);
        }
        geojson::Value::MultiPoint(points) => {
            let _ = mvt_writer.multipoint_begin(0, points.len());
            for point in points {
                let x = point[0];
                let y = point[1];
                let _ = mvt_writer.xy(x, y, 0);
            }
            let _ = mvt_writer.multipoint_end(0);
        }
        geojson::Value::LineString(points) => {
            let _ = mvt_writer.linestring_begin(true, points.len(), 0);
            for point in points {
                let x = point[0];
                let y = point[1];
                let _ = mvt_writer.xy(x, y, 0);
            }
            let _ = mvt_writer.linestring_end(true, 0);
        }
        geojson::Value::MultiLineString(linestrings) => {
            let _ = mvt_writer.multilinestring_begin(linestrings.len(), 0);
            for linestring in linestrings {
                let _ = mvt_writer.linestring_begin(false, linestring.len(), 0);
                for point in linestring {
                    let x = point[0];
                    let y = point[1];
                    let _ = mvt_writer.xy(x, y, 0);
                }
                let _ = mvt_writer.linestring_end(false, 0);
            }
            let _ = mvt_writer.multilinestring_end(0);
        }
        geojson::Value::Polygon(rings) => {
            let _ = mvt_writer.polygon_begin(true, rings.len(), 0);
            for ring in rings {
                let _ = mvt_writer.linestring_begin(false, ring.len(), 0);
                for point in ring {
                    let x = point[0];
                    let y = point[1];
                    let _ = mvt_writer.xy(x, y, 0);
                }
                let _ = mvt_writer.linestring_end(false, 0);
            }
            let _ = mvt_writer.polygon_end(true, 0);
        }
        geojson::Value::MultiPolygon(polygons) => {
            let _ = mvt_writer.multipolygon_begin(polygons.len(), 0);
            for polygon in polygons {
                let _ = mvt_writer.polygon_begin(false, polygon.len(), 0);
                for ring in polygon {
                    let _ = mvt_writer.linestring_begin(false, ring.len(), 0);
                    for point in ring {
                        let x = point[0];
                        let y = point[1];
                        let _ = mvt_writer.xy(x, y, 0);
                    }
                    let _ = mvt_writer.linestring_end(false, 0);
                }
                let _ = mvt_writer.polygon_end(false, 0);
            }
            let _ = mvt_writer.multipolygon_end(0);
        }
        _ => unimplemented!(),
    }
}

/// Helper to update TileJSON bounds.
pub fn update_bounds(bounds: &mut tilejson::Bounds, x: f64, y: f64) {
    bounds.left = f64::min(bounds.left, x);
    bounds.right = f64::max(bounds.right, x);
    bounds.bottom = f64::min(bounds.bottom, y);
    bounds.top = f64::max(bounds.top, y);
}

/// Helper to get bounds from a GeoJSON.
pub fn geojson_to_bounds(geojson_value: &geojson::Value) -> tilejson::Bounds {
    let mut bounds = tilejson::Bounds::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);

    match geojson_value {
        geojson::Value::Point(point) => {
            update_bounds(&mut bounds, point[0], point[1]);
        }
        geojson::Value::MultiPoint(points) => {
            points.iter().for_each(|point| {
                update_bounds(&mut bounds, point[0], point[1]);
            });
        }
        geojson::Value::LineString(points) => {
            points.iter().for_each(|point| {
                update_bounds(&mut bounds, point[0], point[1]);
            });
        }
        geojson::Value::MultiLineString(linestrings) => {
            linestrings.iter().for_each(|linestring| {
                linestring.iter().for_each(|point| {
                    update_bounds(&mut bounds, point[0], point[1]);
                });
            });
        }
        geojson::Value::Polygon(rings) => {
            rings.iter().for_each(|ring| {
                ring.iter().for_each(|point| {
                    update_bounds(&mut bounds, point[0], point[1]);
                });
            });
        }
        geojson::Value::MultiPolygon(polygons) => {
            polygons.iter().for_each(|polygon| {
                polygon.iter().for_each(|ring| {
                    ring.iter().for_each(|point| {
                        update_bounds(&mut bounds, point[0], point[1]);
                    });
                });
            });
        }
        geojson::Value::GeometryCollection(geometries) => {
            for geometry in geometries {
                let col_bounds = geojson_to_bounds(&geometry.value);
                update_bounds(&mut bounds, col_bounds.left, col_bounds.bottom);
                update_bounds(&mut bounds, col_bounds.right, col_bounds.top);
            }
        }
    }

    return bounds;
}

/// Helper function that converts a GeoJSON file into a MVT layer.
pub fn geojson_to_vector_layer(
    layer_name: &str,
    geojson: &geojson::GeoJson,
) -> Vec<tilejson::VectorLayer> {
    let mut fields = BTreeMap::new();
    match geojson {
        geojson::GeoJson::Geometry(_geometry) => {
            vec![tilejson::VectorLayer::new(layer_name.to_string(), fields)]
        }
        geojson::GeoJson::Feature(feature) => {
            if let Some(properties) = feature.properties.as_ref() {
                properties.iter().for_each(|(key, value)| {
                    fields.insert(key.to_string(), property_type(&value).to_string());
                });
            }
            vec![tilejson::VectorLayer::new(layer_name.to_string(), fields)]
        }
        geojson::GeoJson::FeatureCollection(feature_collection) => {
            for feature in &feature_collection.features {
                if let Some(properties) = feature.properties.as_ref() {
                    properties.iter().for_each(|(key, value)| {
                        fields.insert(key.to_string(), property_type(&value).to_string());
                    });
                }
            }
            vec![tilejson::VectorLayer::new(layer_name.to_string(), fields)]
        }
    }
}
