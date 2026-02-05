use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTree, RTreeBuilder};
use geojson::{Feature, FeatureCollection, GeoJson, Geometry, JsonValue, Value};
use geozero::error::GeozeroError;
use geozero::{ColumnValue, FeatureProcessor, GeomProcessor, PropertyProcessor};
use martin_tile_utils::{EARTH_CIRCUMFERENCE, wgs84_to_webmercator};
use serde_json::Map;

use crate::tiles::geojson::source::Rect;

// 1. Filter GeoJSON features - only features that have a geometry can be processed
// 2. Transform geometries from WGS84 to Web Mercator
// 3. Add bounding boxes to R-tree
// 4. Build spatial index for queries
pub(crate) fn preprocess_geojson(geojson: GeoJson) -> (GeoJson, RTree<f64>) {
    match geojson {
        GeoJson::FeatureCollection(mut fc) => {
            // bounding box for entire feature collection
            let mut bbox = Rect::default();
            let transformed_fs = fc
                .features
                .into_iter()
                .filter(|f| f.geometry.is_some())
                .map(|mut f| {
                    let g = transform_geometry(f.geometry.unwrap());
                    // after transform_geometry every geometry is guaranteed to have a bbox
                    if let Some(bb) = &g.bbox {
                        bbox.extend_by_bbox(bb);
                    }
                    f.bbox.clone_from(&g.bbox);
                    f.geometry = Some(g);
                    f
                })
                .collect::<Vec<Feature>>();

            // Build spatial index
            let mut builder = RTreeBuilder::<f64>::new(transformed_fs.len().try_into().unwrap());
            for f in &transformed_fs {
                if let Some(bb) = &f.bbox {
                    builder.add(bb[0], bb[1], bb[2], bb[3]);
                }
            }

            fc.bbox = Some(vec![bbox.min_x, bbox.min_y, bbox.max_x, bbox.max_y]);
            fc.features = transformed_fs;
            let tree = builder.finish::<HilbertSort>();
            (GeoJson::FeatureCollection(fc), tree)
        }
        GeoJson::Feature(mut f) => {
            let count = u32::from(f.geometry.is_some());
            let mut builder = RTreeBuilder::<f64>::new(count);
            let mut fc = FeatureCollection {
                bbox: None,
                features: vec![],
                foreign_members: None,
            };
            if f.geometry.is_some() {
                let transformed_g = transform_geometry(f.geometry.unwrap());
                if let Some(bb) = &transformed_g.bbox {
                    builder.add(bb[0], bb[1], bb[2], bb[3]);
                }
                f.bbox.clone_from(&transformed_g.bbox);
                f.geometry = Some(transformed_g);

                fc.bbox.clone_from(&f.bbox);
                fc.features.push(f);
            }
            let tree = builder.finish::<HilbertSort>();
            (GeoJson::FeatureCollection(fc), tree)
        }
        GeoJson::Geometry(g) => {
            let mut builder = RTreeBuilder::<f64>::new(1);
            let g = transform_geometry(g);
            if let Some(bb) = &g.bbox {
                builder.add(bb[0], bb[1], bb[2], bb[3]);
            }
            let f = Feature {
                bbox: g.bbox.clone(),
                geometry: Some(g),
                id: None,
                properties: None,
                foreign_members: None,
            };
            let fc = FeatureCollection {
                bbox: f.bbox.clone(),
                features: vec![f],
                foreign_members: None,
            };
            let tree = builder.finish::<HilbertSort>();
            (GeoJson::FeatureCollection(fc), tree)
        }
    }
}

/// Transform `GeoJSON` geometry and bounding box from WGS84 to Web Mercator
fn transform_geometry(mut geom: Geometry) -> Geometry {
    match geom.value {
        Value::Point(mut p) => {
            wgs84_to_webmercator_mut_sliced(&mut p);
            let bbox = {
                let rect = Rect::from_position(&p);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::Point(p);
            geom.bbox = Some(bbox);
            geom
        }
        Value::MultiPoint(mut ps) => {
            for p in &mut ps {
                wgs84_to_webmercator_mut_sliced(p);
            }
            let bbox = {
                let rect = Rect::from_positions(&ps);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::MultiPoint(ps);
            geom.bbox = Some(bbox);
            geom
        }
        Value::LineString(mut ps) => {
            for p in &mut ps {
                wgs84_to_webmercator_mut_sliced(p);
            }
            let bbox = {
                let rect = Rect::from_positions(&ps);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::LineString(ps);
            geom.bbox = Some(bbox);
            geom
        }
        Value::MultiLineString(mut ls) => {
            for ps in &mut ls {
                for p in ps {
                    wgs84_to_webmercator_mut_sliced(p);
                }
            }
            let bbox = {
                let rect = Rect::from_linestrings(&ls);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::MultiLineString(ls);
            geom.bbox = Some(bbox);
            geom
        }
        Value::Polygon(mut rs) => {
            for r in &mut rs {
                for p in r {
                    wgs84_to_webmercator_mut_sliced(p);
                }
            }
            let bbox = {
                let rect = Rect::from_linestrings(&rs);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::Polygon(rs);
            geom.bbox = Some(bbox);
            geom
        }
        Value::MultiPolygon(mut ps) => {
            for poly in &mut ps {
                for ring in poly {
                    for p in ring {
                        wgs84_to_webmercator_mut_sliced(p);
                    }
                }
            }
            let bbox = {
                let rect = Rect::from_polygons(&ps);
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::MultiPolygon(ps);
            geom.bbox = Some(bbox);
            geom
        }
        Value::GeometryCollection(gs) => {
            let mut geometries = vec![];
            for g in gs {
                let g = transform_geometry(g);
                geometries.push(g);
            }
            let bbox = {
                let mut rect = Rect::default();
                for g in &geometries {
                    if let Some(bbox) = &g.bbox {
                        rect.extend_by_bbox(bbox);
                    }
                }
                vec![rect.min_x, rect.min_y, rect.max_x, rect.max_y]
            };
            geom.value = Value::GeometryCollection(geometries);
            geom.bbox = Some(bbox);
            geom
        }
    }
}

fn wgs84_to_webmercator_mut_sliced(v: &mut [f64]) {
    assert!(v.len() >= 2);
    let (x, y) = wgs84_to_webmercator(v[0], v[1]);
    v[0] = x;
    v[1] = y;
}

pub(crate) fn tile_length_from_zoom(zoom: u8) -> f64 {
    EARTH_CIRCUMFERENCE / f64::from(1_u32 << zoom)
}

// Processing of GeoJSON features - copy of code from geozero crate since there is no implementation of
// GeozeroDatasource for GeoJson!
// Another solution would be to convert to GeoJsonString and then do the processing, but this would result
// in unnecessary string conversions

/// Process top-level `GeoJSON` items
pub(crate) fn process_geojson<P: FeatureProcessor>(
    gj: &GeoJson,
    processor: &mut P,
) -> Result<(), GeozeroError> {
    match *gj {
        GeoJson::FeatureCollection(ref collection) => {
            processor.dataset_begin(None)?;
            for (idx, feature) in collection.features.iter().enumerate() {
                processor.feature_begin(idx as u64)?;
                if let Some(ref properties) = feature.properties {
                    processor.properties_begin()?;
                    process_properties(properties, processor)?;
                    processor.properties_end()?;
                }
                if let Some(ref geometry) = feature.geometry {
                    processor.geometry_begin()?;
                    process_geojson_geom_n(geometry, idx, processor)?;
                    processor.geometry_end()?;
                }
                processor.feature_end(idx as u64)?;
            }
            processor.dataset_end()
        }
        GeoJson::Feature(ref feature) => process_geojson_feature(feature, 0, processor),
        GeoJson::Geometry(ref geometry) => process_geojson_geom_n(geometry, 0, processor),
    }
}

/// Process top-level `GeoJSON` items
fn process_geojson_feature<P: FeatureProcessor>(
    feature: &Feature,
    idx: usize,
    processor: &mut P,
) -> Result<(), GeozeroError> {
    processor.dataset_begin(None)?;
    if feature.geometry.is_some() || feature.properties.is_some() {
        processor.feature_begin(idx as u64)?;
        if let Some(ref properties) = feature.properties {
            processor.properties_begin()?;
            process_properties(properties, processor)?;
            processor.properties_end()?;
        }
        if let Some(ref geometry) = feature.geometry {
            processor.geometry_begin()?;
            process_geojson_geom_n(geometry, idx, processor)?;
            processor.geometry_end()?;
        }
        processor.feature_end(idx as u64)?;
    }
    processor.dataset_end()
}

/// Process `GeoJSON` geometries
pub(crate) fn process_geojson_geom_n<P: GeomProcessor>(
    geom: &Geometry,
    idx: usize,
    processor: &mut P,
) -> Result<(), GeozeroError> {
    match geom.value {
        Value::Point(ref geometry) => {
            processor.point_begin(idx)?;
            process_coord(geometry, processor.multi_dim(), 0, processor)?;
            processor.point_end(idx)
        }
        Value::MultiPoint(ref geometry) => {
            processor.multipoint_begin(geometry.len(), idx)?;
            let multi_dim = processor.multi_dim();
            for (idxc, point_type) in geometry.iter().enumerate() {
                process_coord(point_type, multi_dim, idxc, processor)?;
            }
            processor.multipoint_end(idx)
        }
        Value::LineString(ref geometry) => process_linestring(geometry, true, idx, processor),
        Value::MultiLineString(ref geometry) => {
            processor.multilinestring_begin(geometry.len(), idx)?;
            for (idx2, linestring_type) in geometry.iter().enumerate() {
                process_linestring(linestring_type, false, idx2, processor)?;
            }
            processor.multilinestring_end(idx)
        }
        Value::Polygon(ref geometry) => process_polygon(geometry, true, idx, processor),
        Value::MultiPolygon(ref geometry) => {
            processor.multipolygon_begin(geometry.len(), idx)?;
            for (idx2, polygon_type) in geometry.iter().enumerate() {
                process_polygon(polygon_type, false, idx2, processor)?;
            }
            processor.multipolygon_end(idx)
        }
        Value::GeometryCollection(ref collection) => {
            processor.geometrycollection_begin(collection.len(), idx)?;
            for (idx2, geometry) in collection.iter().enumerate() {
                process_geojson_geom_n(geometry, idx2, processor)?;
            }
            processor.geometrycollection_end(idx)
        }
    }
}

/// Process `GeoJSON` properties
pub(crate) fn process_properties<P: PropertyProcessor>(
    properties: &Map<String, JsonValue>,
    processor: &mut P,
) -> Result<(), GeozeroError> {
    for (i, (key, value)) in properties.iter().enumerate() {
        // Could we provide a stable property index?
        match value {
            JsonValue::String(v) => processor.property(i, key, &ColumnValue::String(v))?,
            JsonValue::Number(v) => {
                if v.is_f64() {
                    processor.property(i, key, &ColumnValue::Double(v.as_f64().unwrap()))?
                } else if v.is_i64() {
                    processor.property(i, key, &ColumnValue::Long(v.as_i64().unwrap()))?
                } else if v.is_u64() {
                    processor.property(i, key, &ColumnValue::ULong(v.as_u64().unwrap()))?
                } else {
                    unreachable!()
                }
            }
            JsonValue::Bool(v) => processor.property(i, key, &ColumnValue::Bool(*v))?,
            JsonValue::Array(v) => {
                let json_string =
                    serde_json::to_string(v).map_err(|_err| GeozeroError::Property(key.clone()))?;
                processor.property(i, key, &ColumnValue::Json(&json_string))?
            }
            JsonValue::Object(v) => {
                let json_string =
                    serde_json::to_string(v).map_err(|_err| GeozeroError::Property(key.clone()))?;
                processor.property(i, key, &ColumnValue::Json(&json_string))?
            }
            // For null values omit the property
            JsonValue::Null => false,
        };
    }
    Ok(())
}

fn process_coord<P: GeomProcessor>(
    point_type: &[f64],
    multi_dim: bool,
    idx: usize,
    processor: &mut P,
) -> Result<(), GeozeroError> {
    if multi_dim {
        processor.coordinate(
            point_type[0],
            point_type[1],
            point_type.get(2).copied(),
            None,
            None,
            None,
            idx,
        )
    } else {
        processor.xy(point_type[0], point_type[1], idx)
    }
}

fn process_linestring<P: GeomProcessor>(
    linestring_type: &[Vec<f64>],
    tagged: bool,
    idx: usize,
    processor: &mut P,
) -> Result<(), GeozeroError> {
    processor.linestring_begin(tagged, linestring_type.len(), idx)?;
    let multi_dim = processor.multi_dim();
    for (idxc, point_type) in linestring_type.iter().enumerate() {
        process_coord(point_type, multi_dim, idxc, processor)?;
    }
    processor.linestring_end(tagged, idx)
}

fn process_polygon<P: GeomProcessor>(
    polygon_type: &[Vec<Vec<f64>>],
    tagged: bool,
    idx: usize,
    processor: &mut P,
) -> Result<(), GeozeroError> {
    processor.polygon_begin(tagged, polygon_type.len(), idx)?;
    for (idx2, linestring_type) in polygon_type.iter().enumerate() {
        process_linestring(linestring_type, false, idx2, processor)?;
    }
    processor.polygon_end(tagged, idx)
}
