use csscolorparser::{Color, ParseColorError};
use geo_types::Coord;
use geojson::{Feature, FeatureCollection, GeometryValue, JsonObject, Position};
use serde_json::Value as JsonValue;
use tiny_skia::Paint;

use crate::srv::static_overlay::{MarkerOverlay, PathOverlay};

/// Default stroke and fill color when properties don't set one (per simplestyle).
/// Polygons override this for `stroke`, defaulting to their `fill` color instead
/// so a fill-only polygon doesn't render with a surprising contrasting border.
const DEFAULT_COLOR: &str = "#555555";
/// Default stroke width in pixels (per simplestyle).
const DEFAULT_STROKE_WIDTH: f64 = 2.0;
/// Default fill opacity (per simplestyle).
const DEFAULT_FILL_OPACITY: f64 = 0.6;

/// Errors produced while turning a `FeatureCollection` into renderable overlays.
#[derive(Debug, thiserror::Error)]
pub enum OverlayParseError {
    #[error("Invalid CSS color for property {property:?}: {value:?} ({source})")]
    InvalidColor {
        property: &'static str,
        value: String,
        source: ParseColorError,
    },
    #[error("GeoJSON position has fewer than 2 coordinates: {position:?}")]
    PositionTooShort { position: Vec<f64> },
}

/// Renderable overlays extracted from a `GeoJSON` `FeatureCollection`.
#[derive(Debug, Default)]
pub struct ParsedOverlays {
    pub paths: Vec<PathOverlay>,
    pub markers: Vec<MarkerOverlay>,
}

impl ParsedOverlays {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.markers.is_empty()
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
            out.markers
                .push(make_marker(to_coord(coordinates)?, props)?);
        }
        GeometryValue::MultiPoint { coordinates } => {
            for pos in coordinates {
                out.markers.push(make_marker(to_coord(pos)?, props)?);
            }
        }
        GeometryValue::LineString { coordinates } => {
            if let Some(path) = make_path(coordinates, props)? {
                out.paths.push(path);
            }
        }
        GeometryValue::MultiLineString { coordinates } => {
            for line in coordinates {
                if let Some(path) = make_path(line, props)? {
                    out.paths.push(path);
                }
            }
        }
        GeometryValue::Polygon { coordinates } => {
            if let Some(path) = make_polygon(coordinates, props)? {
                out.paths.push(path);
            }
        }
        GeometryValue::MultiPolygon { coordinates } => {
            for polygon in coordinates {
                if let Some(path) = make_polygon(polygon, props)? {
                    out.paths.push(path);
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

/// `GeoJSON` Positions are `[lng, lat, …]` per RFC 7946 § 3.1.1.
/// The geojson crate skips length validation on non-Point Positions, so the bounds check is load-bearing.
fn to_coord(pos: &Position) -> Result<Coord, OverlayParseError> {
    if pos.len() < 2 {
        return Err(OverlayParseError::PositionTooShort {
            position: pos.as_slice().to_vec(),
        });
    }
    Ok(Coord {
        x: pos[0],
        y: pos[1],
    })
}

fn make_path(
    positions: &[Position],
    props: Option<&JsonObject>,
) -> Result<Option<PathOverlay>, OverlayParseError> {
    let Some(points) = ring_coords(positions)? else {
        return Ok(None);
    };
    let (stroke, width) = stroke_paint(props, DEFAULT_COLOR)?;
    Ok(Some(PathOverlay {
        points,
        holes: Vec::new(),
        stroke,
        fill: None,
        width,
    }))
}

fn make_polygon(
    rings: &[Vec<Position>],
    props: Option<&JsonObject>,
) -> Result<Option<PathOverlay>, OverlayParseError> {
    let mut iter = rings.iter();
    let Some(outer) = iter.next() else {
        return Ok(None);
    };
    let Some(points) = ring_coords(outer)? else {
        return Ok(None);
    };
    let mut holes: Vec<Vec<Coord>> = Vec::new();
    for ring in iter {
        if let Some(coords) = ring_coords(ring)? {
            holes.push(coords);
        }
    }
    let fill_color = str_prop(props, "fill").unwrap_or(DEFAULT_COLOR);
    let fill_opacity = f64_prop(props, "fill-opacity").unwrap_or(DEFAULT_FILL_OPACITY);
    let fill = Some(paint_with_opacity("fill", fill_color, fill_opacity)?);
    // Polygon strokes default to the fill color (not gray) so a fill-only
    // polygon renders as a clean shape without a contrasting border.
    let (stroke, width) = stroke_paint(props, fill_color)?;
    Ok(Some(PathOverlay {
        points,
        holes,
        stroke,
        fill,
        width,
    }))
}

/// Convert a ring of `GeoJSON` positions to `Coord`s, dropping rings with < 2 points.
fn ring_coords(positions: &[Position]) -> Result<Option<Vec<Coord>>, OverlayParseError> {
    let coords = positions
        .iter()
        .map(to_coord)
        .collect::<Result<Vec<Coord>, _>>()?;
    Ok((coords.len() >= 2).then_some(coords))
}

/// Resolve simplestyle stroke properties shared by lines and polygons.
/// `default_color` is the color used when the `stroke` property is absent -
/// lines pass [`DEFAULT_COLOR`], polygons pass their resolved `fill` color.
fn stroke_paint(
    props: Option<&JsonObject>,
    default_color: &str,
) -> Result<(Option<Paint<'static>>, Option<f32>), OverlayParseError> {
    let stroke_color = str_prop(props, "stroke").unwrap_or(default_color);
    let stroke_opacity = f64_prop(props, "stroke-opacity").unwrap_or(1.0);
    let stroke = Some(paint_with_opacity("stroke", stroke_color, stroke_opacity)?);

    #[expect(
        clippy::cast_possible_truncation,
        reason = "stroke widths fit in f32 in practice; precision loss is acceptable"
    )]
    let width = Some(f64_prop(props, "stroke-width").unwrap_or(DEFAULT_STROKE_WIDTH) as f32);

    Ok((stroke, width))
}

fn make_marker(
    coord: Coord,
    props: Option<&JsonObject>,
) -> Result<MarkerOverlay, OverlayParseError> {
    let marker_color = match str_prop(props, "marker-color") {
        Some(c) => Some(paint_with_opacity("marker-color", c, 1.0)?),
        None => None,
    };
    Ok(MarkerOverlay {
        coord,
        marker_color,
    })
}

fn str_prop<'a>(props: Option<&'a JsonObject>, key: &str) -> Option<&'a str> {
    props?.get(key).and_then(JsonValue::as_str)
}

fn f64_prop(props: Option<&JsonObject>, key: &str) -> Option<f64> {
    props?.get(key).and_then(JsonValue::as_f64)
}

/// Parse a CSS color and combine it with a `[0.0, 1.0]` opacity multiplier.
///
/// The opacity is multiplied with any alpha already encoded in the color
/// (e.g. `rgba(...)`) so simplestyle-spec's `stroke-opacity: 0.4` correctly
/// dims a fully opaque `stroke: "#312E81"`.
fn paint_with_opacity(
    property: &'static str,
    color: &str,
    opacity: f64,
) -> Result<Paint<'static>, OverlayParseError> {
    let css: Color = color
        .trim()
        .parse()
        .map_err(|source| OverlayParseError::InvalidColor {
            property,
            value: color.to_string(),
            source,
        })?;
    let [r, g, b, base_alpha] = css.to_rgba8();
    let opacity = opacity.clamp(0.0, 1.0);
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "opacity*255 fits in [0, 255]"
    )]
    let alpha = (f64::from(base_alpha) / 255.0 * opacity * 255.0).round() as u8;
    let mut paint = Paint::default();
    paint.set_color_rgba8(r, g, b, alpha);
    paint.anti_alias = true;
    // Force tiny-skia's f32 (HQ) pipeline in debug builds for stable test output;
    // release builds let tiny-skia pick (usually the faster u16 pipeline). The
    // visual difference is imperceptible, so prod takes the speed win.
    paint.force_hq_pipeline = cfg!(debug_assertions);
    Ok(paint)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::{Value, json};

    use super::*;

    fn parse_one(
        properties: &Value,
        geometry: &Value,
    ) -> Result<ParsedOverlays, OverlayParseError> {
        let fc: FeatureCollection = serde_json::from_value(json!({
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
        #[case] expected_paths: usize,
        #[case] expected_markers: usize,
    ) {
        let parsed = parse_one(&json!({}), &geometry).expect("parsing succeeds");
        assert_eq!(parsed.paths.len(), expected_paths, "path count");
        assert_eq!(parsed.markers.len(), expected_markers, "marker count");
    }

    /// `Some(n)` asserts a single path with `n` holes; `None` asserts the polygon was dropped.
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
                assert_eq!(parsed.paths.len(), 1);
                assert_eq!(parsed.paths[0].holes.len(), n);
            }
            None => assert!(parsed.paths.is_empty()),
        }
    }

    #[rstest]
    #[case::default_when_missing(json!({}), 2.0)]
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
        let path = &parsed.paths[0];
        assert_eq!(path.width, Some(expected_width));
        assert!(path.stroke.is_some());
        assert!(path.fill.is_none(), "linestrings never get a fill");
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
        assert!(parsed.paths[0].fill.is_some());
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
        assert!(marker.marker_color.is_some());
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
        assert_eq!(parsed.paths[0].width, Some(4.0));
    }
}
