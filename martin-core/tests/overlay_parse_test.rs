#![cfg(feature = "overlay")]

use martin_core::overlay::{OverlayLayer, OverlayParseError, parse_spec};
use rstest::rstest;
use serde_json::{Value, json};

fn parse(value: Value) -> Result<martin_core::overlay::OverlaySpec, OverlayParseError> {
    parse_spec(&value)
}

fn one_source(features: Value) -> Value {
    json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": features } }
        },
        "layers": []
    })
}

#[test]
fn empty_spec_parses() {
    let spec = parse(json!({})).expect("empty object parses");
    assert!(spec.is_empty());
}

#[test]
fn happy_path_fill_layer() {
    let spec = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{
            "id": "f", "type": "fill", "source": "s",
            "paint": {
                "fill-color": "#ff0000",
                "fill-opacity": 0.4,
                "fill-outline-color": "blue"
            }
        }]
    }))
    .expect("happy path");
    assert_eq!(spec.sources.len(), 1);
    assert_eq!(spec.layers.len(), 1);
    let OverlayLayer::Fill { paint, .. } = &spec.layers[0] else {
        panic!("expected Fill, got {:?}", spec.layers[0]);
    };
    assert!(paint.color.is_some());
    assert!(paint.opacity.is_some());
    assert!(paint.outline_color.is_some());
}

#[test]
fn happy_path_line_layer_with_layout() {
    let spec = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{
            "id": "l", "type": "line", "source": "s",
            "paint": { "line-color": "#000", "line-opacity": 0.9, "line-width": 3 },
            "layout": { "line-cap": "round", "line-join": "miter" }
        }]
    }))
    .expect("happy path");
    let OverlayLayer::Line { paint, layout, .. } = &spec.layers[0] else {
        panic!("expected Line, got {:?}", spec.layers[0]);
    };
    assert!(paint.color.is_some() && paint.width.is_some() && paint.opacity.is_some());
    assert!(layout.cap.is_some() && layout.join.is_some());
}

#[test]
fn happy_path_circle_layer() {
    let spec = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{
            "id": "c", "type": "circle", "source": "s",
            "paint": {
                "circle-color": "red",
                "circle-opacity": 0.5,
                "circle-radius": 8,
                "circle-stroke-color": "#fff",
                "circle-stroke-opacity": 1,
                "circle-stroke-width": 2
            }
        }]
    }))
    .expect("happy path");
    let OverlayLayer::Circle { paint, .. } = &spec.layers[0] else {
        panic!("expected Circle, got {:?}", spec.layers[0]);
    };
    assert!(paint.color.is_some() && paint.radius.is_some());
    assert!(paint.stroke_color.is_some() && paint.stroke_width.is_some());
}

#[test]
fn before_field_survives_parse() {
    let spec = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{
            "id": "f", "type": "fill", "source": "s",
            "before": "base-water"
        }]
    }))
    .expect("happy path");
    let OverlayLayer::Fill { before, .. } = &spec.layers[0] else {
        panic!("expected Fill, got {:?}", spec.layers[0]);
    };
    assert_eq!(before.as_deref(), Some("base-water"));
}

#[test]
fn unknown_top_key_rejected() {
    let err = parse(json!({ "wibbles": 42 })).expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::UnknownTopKey(ref k) if k == "wibbles"),
        "got {err:?}"
    );
}

#[test]
fn url_data_rejected() {
    let err = parse(json!({
        "sources": { "s": { "type": "geojson", "data": "https://example.com/x.geojson" } }
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::SourceDataMustBeInline { ref id } if id == "s"),
        "got {err:?}"
    );
}

#[test]
fn unknown_source_type_rejected() {
    let err = parse(json!({
        "sources": { "s": { "type": "vector", "data": {} } }
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::UnsupportedSourceType { ref id } if id == "s"),
        "got {err:?}"
    );
}

#[test]
fn unknown_layer_type_rejected() {
    let err = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{ "id": "h", "type": "heatmap", "source": "s" }]
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::UnsupportedLayerType { ref id, ref ty }
            if id == "h" && ty == "heatmap"),
        "got {err:?}"
    );
}

#[test]
fn layer_references_missing_source() {
    let err = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{ "id": "f", "type": "fill", "source": "missing" }]
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::UnknownSource { ref id, ref source_id }
            if id == "f" && source_id == "missing"),
        "got {err:?}"
    );
}

#[test]
fn duplicate_layer_id_rejected() {
    let err = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [
            { "id": "x", "type": "fill", "source": "s" },
            { "id": "x", "type": "fill", "source": "s" }
        ]
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::DuplicateId { kind: "layer", ref id } if id == "x"),
        "got {err:?}"
    );
}

#[test]
fn expression_value_rejected() {
    let err = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{
            "id": "l", "type": "line", "source": "s",
            "paint": { "line-width": ["interpolate", ["linear"], ["zoom"], 0, 1, 22, 30] }
        }]
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::ExpressionUnsupported { ref id, prop }
            if id == "l" && prop == "line-width"),
        "got {err:?}"
    );
}

#[test]
fn unknown_paint_key_rejected() {
    let err = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{
            "id": "f", "type": "fill", "source": "s",
            "paint": { "fill-color": "red", "wibble": 1 }
        }]
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::UnknownLayerProp { ref id, section: "paint", ref prop }
            if id == "f" && prop == "wibble"),
        "got {err:?}"
    );
}

#[rstest]
#[case("fill-color", json!("not-a-color"))]
#[case("fill-color", json!("rebeccapurpel"))]
fn invalid_css_color_rejected(#[case] prop: &str, #[case] value: Value) {
    let err = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{
            "id": "f", "type": "fill", "source": "s",
            "paint": { prop: value }
        }]
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::InvalidColor { prop: p, .. } if p == prop),
        "got {err:?}"
    );
}

#[rstest]
#[case::diagonal("diagonal")]
#[case::empty("")]
#[case::wrong_case("BUTT")]
fn invalid_line_cap_rejected(#[case] cap: &str) {
    let err = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{
            "id": "l", "type": "line", "source": "s",
            "layout": { "line-cap": cap }
        }]
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::InvalidLineCap { ref id } if id == "l"),
        "got {err:?}"
    );
}

#[rstest]
#[case::nan(json!(f64::NAN))]
#[case::inf(json!(f64::INFINITY))]
#[case::neg_inf(json!(f64::NEG_INFINITY))]
#[case::string(json!("5"))]
#[case::boolean(json!(true))]
fn invalid_number_rejected(#[case] value: Value) {
    let body = one_source(json!([]));
    // Build a layer with the bad value.
    let mut body = body;
    body["layers"] = json!([{
        "id": "l", "type": "line", "source": "s",
        "paint": { "line-width": value }
    }]);
    let err = parse(body).expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::InvalidNumber { ref id, prop: "line-width" } if id == "l"),
        "got {err:?}"
    );
}

#[test]
fn missing_layer_id_rejected() {
    let err = parse(json!({
        "sources": {
            "s": { "type": "geojson", "data": { "type": "FeatureCollection", "features": [] } }
        },
        "layers": [{ "type": "fill", "source": "s" }]
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::MissingField { field: "id", .. }),
        "got {err:?}"
    );
}

#[test]
fn malformed_geojson_data_rejected() {
    // `"type": "garbage"` is not a valid GeoJSON object type.
    let err = parse(json!({
        "sources": { "s": { "type": "geojson", "data": { "type": "garbage" } } }
    }))
    .expect_err("must error");
    assert!(
        matches!(err, OverlayParseError::MalformedGeoJson { ref id, .. } if id == "s"),
        "got {err:?}"
    );
}
