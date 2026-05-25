//! Deserialization contract for the overlay boundary IR.
//!
//! These tests cover what was previously the `parse_spec` validation pass —
//! now fused into `Deserialize` for [`OverlaySpec`]. They assert alias
//! resolution, canonical-wins precedence, CSS-color/enum/number validation,
//! and `marker-size` translation. The geometry→layer fan-out and simplestyle
//! paint defaults now live in the (maplibre-gated) apply path and are covered
//! by the e2e rendering suite instead.
#![cfg(feature = "overlay")]
#![allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned Value built from json!() macro"
)]

use martin_core::overlay::{LineCap, LineJoin, OverlayProperties, OverlaySpec};
use rstest::rstest;
use serde_json::{Value, json};

fn parse(value: Value) -> Result<OverlaySpec, serde_json::Error> {
    serde_json::from_value(value)
}

/// The validated properties of the single feature in a one-feature spec.
#[track_caller]
fn only_feature_props(spec: &OverlaySpec) -> OverlayProperties {
    assert_eq!(spec.features.len(), 1, "expected exactly one feature");
    spec.features[0].properties.clone().unwrap_or_default()
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

#[test]
fn empty_feature_collection_parses_to_empty_spec() {
    let spec = parse(fc(json!([]))).expect("empty FC parses");
    assert!(spec.is_empty());
}

#[test]
fn no_properties_leaves_all_fields_unset() {
    // Defaults are applied at render time, not in the IR — so a bare point
    // carries no style at all.
    let spec = parse(fc(json!([point(json!({}))]))).expect("parses");
    let props = only_feature_props(&spec);
    assert_eq!(props.circle_color, None);
    assert_eq!(props.circle_radius, None);
    assert_eq!(props.circle_opacity, None);
}

#[test]
fn circle_color_canonical_takes_priority_over_marker_color_alias() {
    let spec = parse(fc(json!([point(
        json!({ "marker-color": "#000000", "circle-color": "#ff0000" })
    )])))
    .expect("parses");
    let color = only_feature_props(&spec).circle_color.expect("color set");
    assert!((color.r - 1.0).abs() < 1e-3, "canonical #ff0000 wins");
}

#[test]
fn marker_color_alias_normalized_to_circle_color() {
    let spec = parse(fc(json!([point(json!({ "marker-color": "#ff0000" }))]))).expect("parses");
    let color = only_feature_props(&spec).circle_color.expect("color set");
    assert!((color.r - 1.0).abs() < 1e-3, "red from marker-color alias");
}

#[rstest]
#[case::small("small", 6.0)]
#[case::medium("medium", 8.0)]
#[case::large("large", 10.0)]
fn marker_size_enum_maps_to_circle_radius(#[case] size: &str, #[case] expected: f32) {
    let spec = parse(fc(json!([point(json!({ "marker-size": size }))]))).expect("parses");
    assert_eq!(only_feature_props(&spec).circle_radius, Some(expected));
}

#[test]
fn invalid_marker_size_enum_rejected() {
    let err = parse(fc(json!([point(json!({ "marker-size": "huge" }))]))).expect_err("rejects");
    assert!(err.to_string().contains("small"), "names valid set: {err}");
}

#[test]
fn circle_radius_canonical_overrides_marker_size_alias() {
    let spec = parse(fc(json!([point(
        json!({ "marker-size": "small", "circle-radius": 99.0 })
    )])))
    .expect("parses");
    assert_eq!(
        only_feature_props(&spec).circle_radius,
        Some(99.0),
        "canonical wins"
    );
}

#[test]
fn circle_stroke_properties_passed_through() {
    let spec = parse(fc(json!([point(json!({
        "circle-stroke-color": "#fff",
        "circle-stroke-opacity": 0.5,
        "circle-stroke-width": 2.0,
    }))])))
    .expect("parses");
    let props = only_feature_props(&spec);
    assert!(props.circle_stroke_color.is_some());
    assert_eq!(props.circle_stroke_opacity, Some(0.5));
    assert_eq!(props.circle_stroke_width, Some(2.0));
}

#[test]
fn stroke_aliases_normalized_to_line_properties() {
    let spec = parse(fc(json!([linestring(
        json!({ "stroke": "#ff0000", "stroke-width": 5, "stroke-opacity": 0.25 })
    )])))
    .expect("parses");
    let props = only_feature_props(&spec);
    let color = props.line_color.expect("line color set");
    assert!((color.r - 1.0).abs() < 1e-3, "red from stroke alias");
    assert_eq!(props.line_width, Some(5.0));
    assert_eq!(props.line_opacity, Some(0.25));
}

#[test]
fn fill_alias_normalized_to_fill_color() {
    let spec = parse(fc(json!([point(json!({ "fill": "#00ff00" }))]))).expect("parses");
    let color = only_feature_props(&spec).fill_color.expect("fill color set");
    assert!((color.g - 1.0).abs() < 1e-3, "green from fill alias");
}

#[test]
fn line_cap_and_line_join_parsed() {
    let spec = parse(fc(json!([linestring(
        json!({ "line-cap": "round", "line-join": "miter" })
    )])))
    .expect("parses");
    let props = only_feature_props(&spec);
    assert_eq!(props.line_cap, Some(LineCap::Round));
    assert_eq!(props.line_join, Some(LineJoin::Miter));
}

#[test]
fn unknown_properties_silently_ignored() {
    // id / name / foo / title / description are not styling properties — they
    // must neither error nor leak into the parsed style.
    let spec = parse(fc(json!([point(json!({
        "id": 42,
        "name": "Antarctica HQ",
        "foo": { "bar": "baz" },
        "title": "Origin",
        "description": "Where the streams cross",
        "circle-color": "red",
    }))])))
    .expect("parses");
    let color = only_feature_props(&spec).circle_color.expect("color set");
    assert!((color.r - 1.0).abs() < 1e-3);
}

#[test]
fn out_of_range_numbers_passed_through_unvalidated() {
    let spec = parse(fc(json!([point(
        json!({ "circle-opacity": 7.0, "circle-radius": -3.0 })
    )])))
    .expect("parses");
    let props = only_feature_props(&spec);
    assert_eq!(props.circle_opacity, Some(7.0), "no range check");
    assert_eq!(props.circle_radius, Some(-3.0), "no range check");
}

#[test]
fn invalid_color_value_rejected() {
    let err = parse(fc(json!([point(
        json!({ "circle-color": "rebeccapurpel" })
    )])))
    .expect_err("rejects");
    assert!(err.to_string().contains("circle-color"), "got {err}");
}

#[rstest]
#[case::diagonal("diagonal")]
#[case::wrong_case("BUTT")]
fn invalid_line_cap_rejected(#[case] cap: &str) {
    let err = parse(fc(json!([linestring(json!({ "line-cap": cap }))]))).expect_err("rejects");
    assert!(err.to_string().contains("butt"), "names valid set: {err}");
}

#[rstest]
#[case::string(json!("5"))]
#[case::boolean(json!(true))]
fn non_numeric_radius_rejected(#[case] value: Value) {
    let err = parse(fc(json!([point(json!({ "circle-radius": value }))]))).expect_err("rejects");
    assert!(err.to_string().contains("f32"), "expects a number: {err}");
}

#[test]
fn null_number_treated_as_absent() {
    // A present `null` is now leniently treated as "unset" rather than a hard
    // error — serde maps it onto the `Option` default.
    let spec = parse(fc(json!([point(json!({ "circle-radius": null }))]))).expect("parses");
    assert_eq!(only_feature_props(&spec).circle_radius, None);
}

#[test]
fn body_not_feature_collection_rejected() {
    // A bare Feature is valid GeoJSON but not what this endpoint accepts.
    let err = parse(json!({
        "type": "Feature",
        "geometry": { "type": "Point", "coordinates": [0, 0] },
        "properties": {}
    }))
    .expect_err("rejects");
    assert!(
        err.to_string().to_lowercase().contains("featurecollection")
            || err.to_string().contains("type"),
        "got {err}"
    );
}

#[test]
fn malformed_body_rejected() {
    let err = parse(json!({ "type": "Wibble" })).expect_err("rejects");
    assert!(!err.to_string().is_empty(), "got an error");
}

#[test]
fn feature_with_null_geometry_kept_with_no_geometry() {
    // Null/unsupported geometries stay in the IR (they are skipped later, at
    // apply time) rather than being dropped during parsing.
    let spec = parse(json!({
        "type": "FeatureCollection",
        "features": [
            { "type": "Feature", "geometry": null, "properties": { "circle-color": "red" } },
            point(json!({ "circle-color": "blue" })),
        ]
    }))
    .expect("parses");
    assert_eq!(spec.features.len(), 2, "both features retained");
    assert!(
        spec.features[0].geometry.is_none(),
        "null geometry parsed as None"
    );
    assert!(spec.features[1].geometry.is_some());
}
