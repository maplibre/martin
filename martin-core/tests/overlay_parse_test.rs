#![cfg(feature = "overlay")]

use martin_core::overlay::{
    OverlayParseError, ParsedOverlays, Shape, Stroke, parse_feature_collection,
};
use rstest::rstest;
use serde_json::{Value, json};

fn parse_one(properties: &Value, geometry: &Value) -> Result<ParsedOverlays, OverlayParseError> {
    let fc: geojson::FeatureCollection = serde_json::from_value(json!({
        "type": "FeatureCollection",
        "features": [{
            "type": "Feature",
            "properties": properties,
            "geometry": geometry,
        }],
    }))
    .expect("valid FeatureCollection");
    parse_feature_collection(&fc)
}

#[rstest]
#[case::point(
    json!({"type": "Point", "coordinates": [0.0, 0.0]}),
    0, 1,
)]
#[case::multipoint_one_marker_per_position(
    json!({"type": "MultiPoint", "coordinates": [[0.0, 0.0], [1.0, 1.0], [2.0, 2.0]]}),
    0, 3,
)]
#[case::linestring(
    json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
    1, 0,
)]
#[case::linestring_with_one_point_is_dropped(
    json!({"type": "LineString", "coordinates": [[0.0, 0.0]]}),
    0, 0,
)]
#[case::geometry_collection_mixes_paths_and_markers(
    json!({"type": "GeometryCollection", "geometries": [
        {"type": "Point", "coordinates": [0.0, 0.0]},
        {"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}
    ]}),
    1, 1,
)]
#[case::null_geometry_is_skipped(json!(null), 0, 0)]
fn geometry_produces_expected_overlay_counts(
    #[case] geometry: Value,
    #[case] expected_shapes: usize,
    #[case] expected_markers: usize,
) {
    let parsed = parse_one(&json!({}), &geometry).expect("parsing succeeds");
    assert_eq!(parsed.shapes.len(), expected_shapes, "shape count");
    assert_eq!(parsed.markers.len(), expected_markers, "marker count");
}

/// `Some(n)` asserts a single polygon with `n` holes; `None` asserts the polygon was dropped.
#[rstest]
#[case::simple_polygon(
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
    ]}),
    Some(0),
)]
#[case::with_valid_hole(
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]],
        [[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.2]]
    ]}),
    Some(1),
)]
#[case::degenerate_hole_dropped_outer_kept(
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]],
        [[0.5, 0.5]]
    ]}),
    Some(0),
)]
#[case::degenerate_outer_drops_polygon(
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0]],
        [[0.2, 0.2], [0.8, 0.2], [0.8, 0.8], [0.2, 0.2]]
    ]}),
    None,
)]
fn polygon_ring_handling(#[case] geometry: Value, #[case] expected_holes: Option<usize>) {
    let parsed = parse_one(&json!({}), &geometry).expect("parsing succeeds");
    match expected_holes {
        Some(n) => {
            assert_eq!(parsed.shapes.len(), 1);
            let Shape::Polygon { holes, .. } = &parsed.shapes[0] else {
                panic!("expected Polygon, got {:?}", parsed.shapes[0]);
            };
            assert_eq!(holes.len(), n);
        }
        None => assert!(parsed.shapes.is_empty()),
    }
}

#[rstest]
#[case::default_when_missing(json!({}), Stroke::DEFAULT_WIDTH)]
#[case::explicit_stroke_width(
    json!({"stroke": "#312E81", "stroke-opacity": 0.4, "stroke-width": 10}),
    10.0,
)]
fn linestring_stroke_width(#[case] properties: Value, #[case] expected_width: f32) {
    let parsed = parse_one(
        &properties,
        &json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
    )
    .expect("parsing succeeds");
    let Shape::Line { stroke, .. } = &parsed.shapes[0] else {
        panic!("expected Line, got {:?}", parsed.shapes[0]);
    };
    assert!((stroke.width - expected_width).abs() < f32::EPSILON);
}

#[test]
fn polygon_renders_with_fill() {
    let parsed = parse_one(
        &json!({"fill": "#ff0000"}),
        &json!({"type": "Polygon", "coordinates": [
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
        ]}),
    )
    .expect("parsing succeeds");
    let Shape::Polygon { fill, .. } = &parsed.shapes[0] else {
        panic!("expected Polygon, got {:?}", parsed.shapes[0]);
    };
    assert!(fill.is_some());
}

#[test]
fn point_uses_marker_color_property() {
    let parsed = parse_one(
        &json!({"marker-color": "#00ff00"}),
        &json!({"type": "Point", "coordinates": [10.0, 20.0]}),
    )
    .expect("parsing succeeds");
    let marker = &parsed.markers[0];
    assert!((marker.coord.x - 10.0).abs() < f64::EPSILON);
    assert!((marker.coord.y - 20.0).abs() < f64::EPSILON);
    // green
    assert_eq!(marker.style.color.r, 0);
    assert_eq!(marker.style.color.g, 255);
    assert_eq!(marker.style.color.b, 0);
}

#[rstest]
#[case::stroke_typo(
    json!({"stroke": "blu"}),
    json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
    "stroke",
    "blu",
)]
#[case::fill_typo(
    json!({"fill": "rebeccapurpel"}),
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
    ]}),
    "fill",
    "rebeccapurpel",
)]
#[case::marker_typo(
    json!({"marker-color": "blu"}),
    json!({"type": "Point", "coordinates": [0.0, 0.0]}),
    "marker-color",
    "blu",
)]
fn invalid_css_color_reports_offending_property(
    #[case] properties: Value,
    #[case] geometry: Value,
    #[case] expected_property: &str,
    #[case] expected_value: &str,
) {
    let err = parse_one(&properties, &geometry).expect_err("invalid color must error");
    let OverlayParseError::InvalidColor {
        property, value, ..
    } = &err
    else {
        panic!("expected InvalidColor, got {err:?}");
    };
    assert_eq!(*property, expected_property);
    assert_eq!(value, expected_value);
    let msg = err.to_string();
    assert!(msg.contains(expected_property), "{msg}");
    assert!(msg.contains(expected_value), "{msg}");
}

#[rstest]
#[case::stroke_width_as_string(
    json!({"stroke-width": "5"}),
    json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
    "stroke-width",
    json!("5"),
)]
#[case::stroke_opacity_as_string(
    json!({"stroke-opacity": "0.4"}),
    json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
    "stroke-opacity",
    json!("0.4"),
)]
#[case::stroke_width_as_bool(
    json!({"stroke-width": true}),
    json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
    "stroke-width",
    json!(true),
)]
#[case::fill_opacity_as_string(
    json!({"fill-opacity": "0.5"}),
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
    ]}),
    "fill-opacity",
    json!("0.5"),
)]
#[case::polygon_stroke_width_as_string(
    json!({"fill": "#ff0000", "stroke-width": "3"}),
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
    ]}),
    "stroke-width",
    json!("3"),
)]
fn non_numeric_property_errors_instead_of_silently_defaulting(
    #[case] properties: Value,
    #[case] geometry: Value,
    #[case] expected_property: &str,
    #[case] expected_value: Value,
) {
    let err = parse_one(&properties, &geometry)
        .expect_err("non-numeric numeric property must error, not silently default");
    let OverlayParseError::NonNumericProperty { property, value } = &err else {
        panic!("expected NonNumericProperty, got {err:?}");
    };
    assert_eq!(*property, expected_property);
    assert_eq!(*value, expected_value);
}

#[rstest]
#[case::stroke_width_null(
    json!({"stroke-width": null}),
    json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
)]
#[case::stroke_opacity_null(
    json!({"stroke-opacity": null}),
    json!({"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}),
)]
#[case::fill_opacity_null(
    json!({"fill-opacity": null}),
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]]
    ]}),
)]
fn explicit_null_is_treated_as_absent(#[case] properties: Value, #[case] geometry: Value) {
    parse_one(&properties, &geometry).expect("explicit null falls back to default");
}

/// Point is excluded — the geojson crate rejects short Point coordinates at deserialize time.
#[rstest]
#[case::multipoint(
    json!({"type": "MultiPoint", "coordinates": [[1.0]]}),
)]
#[case::linestring(
    json!({"type": "LineString", "coordinates": [[1.0], [2.0]]}),
)]
#[case::polygon_outer_ring(
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0], [1.0, 1.0], [0.0, 0.0]]
    ]}),
)]
#[case::polygon_hole(
    json!({"type": "Polygon", "coordinates": [
        [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 0.0]],
        [[0.5, 0.5], [0.5], [0.6, 0.6], [0.5, 0.5]]
    ]}),
)]
#[case::geometry_collection_nested_short_linestring(
    json!({"type": "GeometryCollection", "geometries": [
        {"type": "LineString", "coordinates": [[1.0], [2.0]]}
    ]}),
)]
fn short_position_errors_instead_of_panicking(#[case] geometry: Value) {
    let err = parse_one(&json!({}), &geometry).expect_err("short position must error, not panic");
    assert!(
        matches!(err, OverlayParseError::PositionTooShort { .. }),
        "expected PositionTooShort, got {err:?}",
    );
}

#[test]
fn geometry_collection_inherits_parent_properties() {
    let parsed = parse_one(
        &json!({"stroke": "#ff0000", "stroke-width": 4}),
        &json!({"type": "GeometryCollection", "geometries": [
            {"type": "Point", "coordinates": [0.0, 0.0]},
            {"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]}
        ]}),
    )
    .expect("parsing succeeds");
    let Shape::Line { stroke, .. } = &parsed.shapes[0] else {
        panic!("expected Line, got {:?}", parsed.shapes[0]);
    };
    assert!((stroke.width - 4.0).abs() < f32::EPSILON);
}

/// Wrap `inner` in `depth` levels of `GeometryCollection`.
fn nested_gc_feature_collection_json(depth: usize, inner: &str) -> String {
    let mut geometry = inner.to_string();
    for _ in 0..depth {
        geometry = format!(r#"{{"type":"GeometryCollection","geometries":[{geometry}]}}"#);
    }
    format!(
        r#"{{"type":"FeatureCollection","features":[{{"type":"Feature","geometry":{geometry},"properties":{{}}}}]}}"#
    )
}

/// Deeply nested `GeometryCollection` payloads must error at JSON parse
/// time (via `serde_json`'s `RECURSION_LIMIT`, 128) rather than recursing
/// through `push_geometry` and overflowing the worker thread's stack.
/// `OverlayBody::from_request` calls `serde_json::from_slice`, so this
/// test mirrors that entry point.
#[test]
fn deeply_nested_geometry_collection_errors_at_parse_time() {
    // 256 GC layers ≈ 768 JSON nesting layers — well past the 128 limit.
    let body = nested_gc_feature_collection_json(256, r#"{"type":"Point","coordinates":[0,0]}"#);
    let result: Result<geojson::FeatureCollection, _> = serde_json::from_str(&body);
    assert!(
        result.is_err(),
        "deeply nested GeometryCollection must error at parse, not stack-overflow"
    );
}

/// Within `serde_json`'s recursion limit, nested `GeometryCollection`s
/// must walk through `push_geometry` without panicking and surface the
/// innermost geometry.
#[test]
fn moderately_nested_geometry_collection_walks_without_panicking() {
    // ~20 GC layers ≈ 60 JSON layers, comfortably under the 128 limit.
    let body = nested_gc_feature_collection_json(20, r#"{"type":"Point","coordinates":[0,0]}"#);
    let fc: geojson::FeatureCollection =
        serde_json::from_str(&body).expect("moderate nesting parses");
    let parsed = parse_feature_collection(&fc).expect("walks without panicking");
    assert_eq!(parsed.markers.len(), 1, "innermost point reaches markers");
    assert_eq!(parsed.shapes.len(), 0);
}
