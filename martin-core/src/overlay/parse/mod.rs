//! Build [`ParsedOverlays`] from a `GeoJSON` `FeatureCollection`.
//!
//! The walker dispatches on `GeometryValue`; per-shape construction lives in
//! [`geometry`] and simplestyle property handling in [`simplestyle`].

mod geometry;
mod simplestyle;

use csscolorparser::ParseColorError;
use geojson::{Feature, FeatureCollection, GeometryValue, JsonObject};

use crate::overlay::parse::geometry::{make_line, make_marker, make_polygon, to_coord};
use crate::overlay::{Marker, Shape};

/// Errors produced while turning a `FeatureCollection` into renderable overlays.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum OverlayParseError {
    /// A simplestyle color property (`stroke`, `fill`, `marker-color`) did not
    /// parse as a CSS color.
    #[error("Invalid CSS color for property {property:?}: {value:?} ({source})")]
    InvalidColor {
        /// The simplestyle property whose value failed to parse.
        property: &'static str,
        /// The raw value that failed to parse, echoed back for diagnostics.
        value: String,
        /// The underlying `csscolorparser` error.
        source: ParseColorError,
    },
    /// A `GeoJSON` Position had fewer than 2 coordinates. RFC 7946 § 3.1.1
    /// requires at least `[lon, lat]`.
    #[error("GeoJSON position has fewer than 2 coordinates: {position:?}")]
    PositionTooShort {
        /// The offending Position array, echoed back for diagnostics.
        position: Vec<f64>,
    },
}

/// Renderable overlays extracted from a `GeoJSON` `FeatureCollection`.
#[derive(Debug, Default)]
pub struct ParsedOverlays {
    /// Path overlays (line strings and polygons), in input order.
    pub shapes: Vec<Shape>,
    /// Marker overlays (points), in input order.
    pub markers: Vec<Marker>,
}

impl ParsedOverlays {
    /// Returns `true` when there is nothing to draw.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.shapes.is_empty() && self.markers.is_empty()
    }
}

/// Walk a `FeatureCollection` and build path/marker overlays from its features.
pub fn parse_feature_collection(
    fc: &FeatureCollection,
) -> Result<ParsedOverlays, OverlayParseError> {
    let mut out = ParsedOverlays::default();
    for feature in &fc.features {
        push_feature(feature, &mut out)?;
    }
    Ok(out)
}

fn push_feature(feature: &Feature, out: &mut ParsedOverlays) -> Result<(), OverlayParseError> {
    let Some(geom) = feature.geometry.as_ref() else {
        return Ok(());
    };
    push_geometry(&geom.value, feature.properties.as_ref(), out)
}

fn push_geometry(
    value: &GeometryValue,
    props: Option<&JsonObject>,
    out: &mut ParsedOverlays,
) -> Result<(), OverlayParseError> {
    match value {
        GeometryValue::Point { coordinates } => {
            out.markers.push(make_marker(to_coord(coordinates)?, props)?);
        }
        GeometryValue::MultiPoint { coordinates } => {
            for pos in coordinates {
                out.markers.push(make_marker(to_coord(pos)?, props)?);
            }
        }
        GeometryValue::LineString { coordinates } => {
            if let Some(shape) = make_line(coordinates, props)? {
                out.shapes.push(shape);
            }
        }
        GeometryValue::MultiLineString { coordinates } => {
            for line in coordinates {
                if let Some(shape) = make_line(line, props)? {
                    out.shapes.push(shape);
                }
            }
        }
        GeometryValue::Polygon { coordinates } => {
            if let Some(shape) = make_polygon(coordinates, props)? {
                out.shapes.push(shape);
            }
        }
        GeometryValue::MultiPolygon { coordinates } => {
            for polygon in coordinates {
                if let Some(shape) = make_polygon(polygon, props)? {
                    out.shapes.push(shape);
                }
            }
        }
        GeometryValue::GeometryCollection { geometries } => {
            // Properties on the parent feature apply to every nested geometry.
            for geom in geometries {
                push_geometry(&geom.value, props, out)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::{Value, json};

    use crate::overlay::parse::{OverlayParseError, ParsedOverlays, parse_feature_collection};
    use crate::overlay::{Shape, Stroke};

    fn parse_one(
        properties: &Value,
        geometry: &Value,
    ) -> Result<ParsedOverlays, OverlayParseError> {
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
        let err =
            parse_one(&json!({}), &geometry).expect_err("short position must error, not panic");
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
}
