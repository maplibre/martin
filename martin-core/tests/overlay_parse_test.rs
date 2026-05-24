#![cfg(feature = "overlay")]
#![allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned Value built from json!() macro"
)]

use martin_core::overlay::{OverlayLayer, OverlayParseError, parse_spec};
use rstest::rstest;
use serde_json::{Value, json};

/// `#7e7e7e` channel value as `f32` — `u8` cast is lossless.
const GRAY_7E: f32 = 0x7e_u8 as f32 / 255.0;
/// `#555555` channel value as `f32`.
const GRAY_55: f32 = 0x55_u8 as f32 / 255.0;

fn parse(value: Value) -> Result<martin_core::overlay::OverlaySpec, OverlayParseError> {
    parse_spec(&value)
}

fn fc(features: Value) -> Value {
    json!({ "type": "FeatureCollection", "features": features })
}

fn point(props: Value) -> Value {
    json!({
        "type": "Feature",
        "geometry": { "type": "Point", "coordinates": [0.0, 0.0] },
        "properties": props,
    })
}

fn linestring(props: Value) -> Value {
    json!({
        "type": "Feature",
        "geometry": { "type": "LineString", "coordinates": [[-1.0, -1.0], [1.0, 1.0]] },
        "properties": props,
    })
}

fn polygon(props: Value) -> Value {
    json!({
        "type": "Feature",
        "geometry": {
            "type": "Polygon",
            "coordinates": [[[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0], [-1.0, -1.0]]]
        },
        "properties": props,
    })
}

#[test]
fn empty_feature_collection_parses_to_empty_spec() {
    let spec = parse(fc(json!([]))).expect("empty FC parses");
    assert!(spec.is_empty());
}

#[test]
fn point_with_no_properties_gets_default_circle() {
    let spec = parse(fc(json!([point(json!({}))]))).expect("parses");
    assert_eq!(spec.sources.len(), 1);
    assert_eq!(spec.layers.len(), 1);
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        panic!("expected Circle, got {:?}", spec.layers[0]);
    };
    let color = paint.color.expect("color defaulted");
    assert!(
        (color.r - GRAY_7E).abs() < 1e-3,
        "circle-color default #7e7e7e"
    );
    assert_eq!(paint.radius, Some(8.0), "circle-radius default 8");
    assert_eq!(paint.opacity, Some(1.0), "circle-opacity default 1.0");
}

#[test]
fn circle_color_canonical_takes_priority_over_marker_color_alias() {
    let spec = parse(fc(json!([point(
        json!({ "marker-color": "#000000", "circle-color": "#ff0000" })
    )])))
    .expect("parses");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    let color = paint.color.unwrap();
    assert!((color.r - 1.0).abs() < 1e-3, "canonical #ff0000 wins");
}

#[test]
fn marker_color_alias_normalized_to_circle_color() {
    let spec = parse(fc(json!([point(json!({ "marker-color": "#ff0000" }))]))).expect("parses");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    let color = paint.color.unwrap();
    assert!((color.r - 1.0).abs() < 1e-3, "red from marker-color alias");
}

#[rstest]
#[case::small("small", 6.0)]
#[case::medium("medium", 8.0)]
#[case::large("large", 10.0)]
fn marker_size_enum_maps_to_circle_radius(#[case] size: &str, #[case] expected: f32) {
    let spec = parse(fc(json!([point(json!({ "marker-size": size }))]))).expect("parses");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    assert_eq!(paint.radius, Some(expected));
}

#[test]
fn invalid_marker_size_enum_rejected() {
    let err = parse(fc(json!([point(json!({ "marker-size": "huge" }))]))).expect_err("rejects");
    assert!(
        matches!(err, OverlayParseError::InvalidMarkerSize { index: 0 }),
        "got {err:?}"
    );
}

#[test]
fn circle_radius_canonical_overrides_marker_size_alias() {
    let spec = parse(fc(json!([point(
        json!({ "marker-size": "small", "circle-radius": 99.0 })
    )])))
    .expect("parses");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    assert_eq!(paint.radius, Some(99.0), "canonical wins");
}

#[test]
fn circle_stroke_properties_passed_through() {
    let spec = parse(fc(json!([point(json!({
        "circle-stroke-color": "#fff",
        "circle-stroke-opacity": 0.5,
        "circle-stroke-width": 2.0,
    }))])))
    .expect("parses");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    assert!(paint.stroke_color.is_some());
    assert_eq!(paint.stroke_opacity, Some(0.5));
    assert_eq!(paint.stroke_width, Some(2.0));
}

#[test]
fn linestring_with_no_properties_gets_default_line() {
    let spec = parse(fc(json!([linestring(json!({}))]))).expect("parses");
    assert_eq!(spec.layers.len(), 1);
    let OverlayLayer::Line { paint, .. } = &spec.layers[0] else {
        panic!("expected Line, got {:?}", spec.layers[0]);
    };
    let color = paint.color.unwrap();
    assert!((color.r - GRAY_55).abs() < 1e-3, "line-color default #555");
    assert_eq!(paint.width, Some(2.0), "line-width default 2");
    assert_eq!(paint.opacity, Some(1.0));
}

#[test]
fn stroke_alias_normalized_to_line_color_on_linestring() {
    let spec = parse(fc(json!([linestring(
        json!({ "stroke": "#ff0000", "stroke-width": 5 })
    )])))
    .expect("parses");
    let OverlayLayer::Line { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    let color = paint.color.unwrap();
    assert!((color.r - 1.0).abs() < 1e-3, "red from stroke alias");
    assert_eq!(paint.width, Some(5.0));
}

#[test]
fn line_cap_and_line_join_layout_parsed() {
    let spec = parse(fc(json!([linestring(
        json!({ "line-cap": "round", "line-join": "miter" })
    )])))
    .expect("parses");
    let OverlayLayer::Line { layout, .. } = &spec.layers[0] else {
        unreachable!()
    };
    assert!(matches!(
        layout.cap,
        Some(martin_core::overlay::LineCap::Round)
    ));
    assert!(matches!(
        layout.join,
        Some(martin_core::overlay::LineJoin::Miter)
    ));
}

#[test]
fn polygon_with_no_properties_gets_default_fill_only() {
    let spec = parse(fc(json!([polygon(json!({}))]))).expect("parses");
    assert_eq!(spec.layers.len(), 1, "bare polygon → fill only");
    let OverlayLayer::Fill { paint, .. } = &spec.layers[0] else {
        panic!("expected Fill, got {:?}", spec.layers[0]);
    };
    let color = paint.color.unwrap();
    assert!((color.r - GRAY_55).abs() < 1e-3, "fill-color default #555");
    assert_eq!(paint.opacity, Some(0.6), "fill-opacity default 0.6");
}

#[test]
fn polygon_with_only_fill_emits_fill_layer_only() {
    let spec = parse(fc(json!([polygon(
        json!({ "fill": "green", "fill-opacity": 1.0 })
    )])))
    .expect("parses");
    assert_eq!(spec.layers.len(), 1);
    assert!(matches!(spec.layers[0], OverlayLayer::Fill { .. }));
}

#[test]
fn hollow_polygon_with_only_stroke_emits_line_layer_only() {
    let spec = parse(fc(json!([polygon(
        json!({ "stroke": "darkgreen", "stroke-width": 2 })
    )])))
    .expect("parses");
    assert_eq!(spec.layers.len(), 1, "hollow polygon → line only");
    assert!(matches!(spec.layers[0], OverlayLayer::Line { .. }));
}

#[test]
fn polygon_with_fill_and_stroke_emits_both_layers() {
    let spec = parse(fc(json!([polygon(json!({
        "fill": "green",
        "stroke": "darkgreen",
        "stroke-width": 2
    }))])))
    .expect("parses");
    assert_eq!(spec.layers.len(), 2);
    assert!(
        matches!(spec.layers[0], OverlayLayer::Fill { .. }),
        "fill first"
    );
    assert!(
        matches!(spec.layers[1], OverlayLayer::Line { .. }),
        "line second"
    );
}

#[test]
fn multiple_features_get_independent_sources_and_layers() {
    let spec = parse(fc(json!([
        point(json!({ "circle-color": "red" })),
        linestring(json!({ "line-color": "blue" })),
        polygon(json!({ "fill-color": "green", "line-color": "darkgreen" })),
    ])))
    .expect("parses");
    assert_eq!(spec.sources.len(), 3, "one source per feature");
    assert_eq!(spec.layers.len(), 4, "circle + line + fill + line");
    assert!(matches!(spec.layers[0], OverlayLayer::Circle { .. }));
    assert!(matches!(spec.layers[1], OverlayLayer::Line { .. }));
    assert!(matches!(spec.layers[2], OverlayLayer::Fill { .. }));
    assert!(matches!(spec.layers[3], OverlayLayer::Line { .. }));
}

#[test]
fn unknown_properties_silently_ignored() {
    // id / name / foo are not styling properties — should not error and
    // should not affect the emitted paint.
    let spec = parse(fc(json!([point(json!({
        "id": 42,
        "name": "Antarctica HQ",
        "foo": { "bar": "baz" },
        "circle-color": "red",
    }))])))
    .expect("parses");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    let color = paint.color.unwrap();
    assert!((color.r - 1.0).abs() < 1e-3);
}

#[test]
fn title_and_description_ignored_for_rendering() {
    // simplestyle's informational fields are dropped at the render stage.
    let spec = parse(fc(json!([point(json!({
        "title": "Origin",
        "description": "Where the streams cross",
    }))])))
    .expect("parses");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    // Falls through to defaults — no error and no surprise styling.
    let color = paint.color.unwrap();
    assert!((color.r - GRAY_7E).abs() < 1e-3);
}

#[test]
fn opacity_above_one_passed_through_unvalidated() {
    let spec = parse(fc(json!([point(json!({ "circle-opacity": 7.0 }))]))).expect("parses");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    assert_eq!(paint.opacity, Some(7.0), "no range check");
}

#[test]
fn negative_line_width_passed_through_unvalidated() {
    let spec = parse(fc(json!([linestring(json!({ "line-width": -3.0 }))]))).expect("parses");
    let OverlayLayer::Line { paint, .. } = &spec.layers[0] else {
        unreachable!()
    };
    assert_eq!(paint.width, Some(-3.0), "no range check");
}

#[test]
fn invalid_color_value_rejected() {
    let err = parse(fc(json!([point(
        json!({ "circle-color": "rebeccapurpel" })
    )])))
    .expect_err("rejects");
    assert!(
        matches!(
            err,
            OverlayParseError::InvalidColor {
                index: 0,
                prop: "circle-color",
                ..
            }
        ),
        "got {err:?}"
    );
}

#[rstest]
#[case::diagonal("diagonal")]
#[case::wrong_case("BUTT")]
fn invalid_line_cap_rejected(#[case] cap: &str) {
    let err = parse(fc(json!([linestring(json!({ "line-cap": cap }))]))).expect_err("rejects");
    assert!(
        matches!(err, OverlayParseError::InvalidLineCap { index: 0 }),
        "got {err:?}"
    );
}

#[rstest]
#[case::string(json!("5"))]
#[case::boolean(json!(true))]
#[case::null(json!(null))]
fn non_numeric_radius_rejected(#[case] value: Value) {
    let mut props = serde_json::Map::new();
    props.insert("circle-radius".to_string(), value);
    let err = parse(fc(json!([point(Value::Object(props))]))).expect_err("rejects");
    assert!(
        matches!(
            err,
            OverlayParseError::InvalidNumber {
                index: 0,
                prop: "circle-radius"
            }
        ),
        "got {err:?}"
    );
}

#[test]
fn body_not_feature_collection_rejected() {
    // A bare Feature (not wrapped in a FeatureCollection) is a GeoJSON
    // object, but not what this endpoint accepts.
    let err = parse(json!({
        "type": "Feature",
        "geometry": { "type": "Point", "coordinates": [0, 0] },
        "properties": {}
    }))
    .expect_err("rejects");
    assert!(
        matches!(
            err,
            OverlayParseError::NotFeatureCollection { actual: "Feature" }
        ),
        "got {err:?}"
    );
}

#[test]
fn malformed_body_rejected() {
    let err = parse(json!({ "type": "Wibble" })).expect_err("rejects");
    assert!(
        matches!(err, OverlayParseError::MalformedGeoJson { .. }),
        "got {err:?}"
    );
}

#[test]
fn feature_with_null_geometry_skipped() {
    let spec = parse(json!({
        "type": "FeatureCollection",
        "features": [
            { "type": "Feature", "geometry": null, "properties": { "circle-color": "red" } },
            point(json!({ "circle-color": "blue" })),
        ]
    }))
    .expect("parses");
    assert_eq!(spec.sources.len(), 1, "null geometry skipped");
    assert_eq!(spec.layers.len(), 1);
}
