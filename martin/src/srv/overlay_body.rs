//! Wire format for static-render overlays.
//!
//! A `GeoJSON` `FeatureCollection` arrives on the request body and is
//! deserialized here into martin-core's typed [`OverlaySpec`]. Deserialization
//! is an application concern, so all of it lives in this crate, not in
//! martin-core - the core only ever sees the already-validated IR.
//!
//! These same structs are the `OpenAPI` request-body schema (via `utoipa`'s
//! `ToSchema`, gated behind `unstable-schemas`), so the documented schema can
//! never drift from what the parser actually accepts.
//!
//! serde validates the envelope, the enums, and the numbers;
//! [`StaticOverlayProperties`] then maps the canonical `MapLibre` property names
//! onto the [`OverlayProperties`] fields and parses the CSS color strings. A bad
//! value surfaces as an error string, mapped to a 400 at the HTTP boundary.

// Compiled standalone under the maplibre-free `overlay` feature so the parser
// and its tests build without maplibre. The only non-test caller is
// `styles_static` (rendering + linux), so dead-code is only meaningful - and
// thus only enforced - in that configuration.
#![cfg_attr(not(all(feature = "rendering", target_os = "linux")), allow(dead_code))]

use martin_core::overlay::{
    Color, LineCap, LineJoin, OverlayFeature, OverlayProperties, OverlaySpec,
};
use serde::Deserialize;

/// Parse a POST overlay body into the typed [`OverlaySpec`].
///
/// # Errors
///
/// Returns a human-readable message on malformed JSON, a non-`FeatureCollection`
/// body, an invalid enum/number, or an unparseable CSS color.
pub(crate) fn parse_overlay(bytes: &[u8]) -> Result<OverlaySpec, String> {
    let raw: StaticStyleOverlay =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid JSON overlay body: {e}"))?;
    OverlaySpec::try_from(raw)
}

/// `"FeatureCollection"` discriminator for the top-level body. A unit enum so
/// any other `type` (a bare `Feature`, `Geometry`, or garbage) fails to
/// deserialize with a clear serde error.
#[derive(Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
enum FeatureCollectionTag {
    FeatureCollection,
}

/// `"Feature"` discriminator for each member of the collection.
#[derive(Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
enum FeatureTag {
    Feature,
}

/// Wire shape of the top-level body: a `GeoJSON` `FeatureCollection`. Doubles as
/// the `OpenAPI` request-body schema.
#[derive(Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
pub(crate) struct StaticStyleOverlay {
    /// `GeoJSON` type discriminator. Must be `"FeatureCollection"`.
    #[serde(rename = "type")]
    #[expect(dead_code, reason = "validated by Deserialize, then discarded")]
    tag: FeatureCollectionTag,
    /// Features to overlay on the rendered base map, in draw order.
    features: Vec<StaticOverlayFeature>,
}

impl TryFrom<StaticStyleOverlay> for OverlaySpec {
    type Error = String;

    fn try_from(raw: StaticStyleOverlay) -> Result<Self, Self::Error> {
        let features = raw
            .features
            .into_iter()
            .map(OverlayFeature::try_from)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { features })
    }
}

/// Wire shape of one `GeoJSON` `Feature`. A `null`/missing geometry is kept as
/// `None` and skipped at apply time.
#[derive(Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
struct StaticOverlayFeature {
    /// `GeoJSON` type discriminator. Must be `"Feature"`.
    #[serde(rename = "type")]
    #[expect(dead_code, reason = "validated by Deserialize, then discarded")]
    tag: FeatureTag,
    /// `GeoJSON` geometry. `Point`/`MultiPoint` → circle layer;
    /// `LineString`/`MultiLineString` → line layer;
    /// `Polygon`/`MultiPolygon` → fill (and optionally outline-line) layer.
    /// `GeometryCollection` and `null` are silently skipped.
    #[serde(default)]
    #[cfg_attr(feature = "unstable-schemas", schema(value_type = Option<serde_json::Value>))]
    geometry: Option<geojson::Geometry>,
    /// Styling for this feature. All fields optional; unknown fields ignored.
    #[serde(default)]
    properties: Option<StaticOverlayProperties>,
}

impl TryFrom<StaticOverlayFeature> for OverlayFeature {
    type Error = String;

    fn try_from(raw: StaticOverlayFeature) -> Result<Self, Self::Error> {
        Ok(Self {
            geometry: raw.geometry,
            properties: raw
                .properties
                .map(OverlayProperties::try_from)
                .transpose()?,
        })
    }
}

/// Wire `line-cap` value; lowercase strings folded into the core [`LineCap`].
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum WireLineCap {
    Butt,
    Round,
    Square,
}

impl From<WireLineCap> for LineCap {
    fn from(cap: WireLineCap) -> Self {
        match cap {
            WireLineCap::Butt => Self::Butt,
            WireLineCap::Round => Self::Round,
            WireLineCap::Square => Self::Square,
        }
    }
}

/// Wire `line-join` value; lowercase strings folded into the core [`LineJoin`].
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum WireLineJoin {
    Miter,
    Bevel,
    Round,
}

impl From<WireLineJoin> for LineJoin {
    fn from(join: WireLineJoin) -> Self {
        match join {
            WireLineJoin::Miter => Self::Miter,
            WireLineJoin::Bevel => Self::Bevel,
            WireLineJoin::Round => Self::Round,
        }
    }
}

/// Wire shape of a feature's `properties`, keyed by canonical `MapLibre`
/// paint/layout names. Colors arrive as strings and are parsed in [`TryFrom`];
/// enums and numbers are validated by serde. Unknown keys (`id`, `name`,
/// `title`, …) are ignored.
#[derive(Default, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "kebab-case")]
struct StaticOverlayProperties {
    /// CSS color for `Point` geometries.
    #[cfg_attr(feature = "unstable-schemas", schema(example = "#285DAA"))]
    circle_color: Option<String>,
    circle_opacity: Option<f32>,
    /// Radius in pixels at the rendered scale.
    #[cfg_attr(feature = "unstable-schemas", schema(example = 8.0))]
    circle_radius: Option<f32>,
    circle_stroke_color: Option<String>,
    circle_stroke_opacity: Option<f32>,
    circle_stroke_width: Option<f32>,

    /// CSS color for `LineString` geometries (and `Polygon` outlines).
    #[cfg_attr(feature = "unstable-schemas", schema(example = "#285DAA"))]
    line_color: Option<String>,
    line_opacity: Option<f32>,
    /// Line width in pixels at the rendered scale.
    #[cfg_attr(feature = "unstable-schemas", schema(example = 2.0))]
    line_width: Option<f32>,
    /// One of `butt`, `round`, `square`.
    #[cfg_attr(feature = "unstable-schemas", schema(value_type = Option<String>, example = "round"))]
    line_cap: Option<WireLineCap>,
    /// One of `miter`, `bevel`, `round`.
    #[cfg_attr(feature = "unstable-schemas", schema(value_type = Option<String>, example = "round"))]
    line_join: Option<WireLineJoin>,

    /// CSS color for `Polygon` fills.
    #[cfg_attr(feature = "unstable-schemas", schema(example = "#95BEFA"))]
    fill_color: Option<String>,
    fill_opacity: Option<f32>,
    fill_outline_color: Option<String>,
}

impl TryFrom<StaticOverlayProperties> for OverlayProperties {
    type Error = String;

    fn try_from(raw: StaticOverlayProperties) -> Result<Self, Self::Error> {
        Ok(Self {
            circle_color: parse_color("circle-color", raw.circle_color)?,
            circle_opacity: raw.circle_opacity,
            circle_radius: raw.circle_radius,
            circle_stroke_color: parse_color("circle-stroke-color", raw.circle_stroke_color)?,
            circle_stroke_opacity: raw.circle_stroke_opacity,
            circle_stroke_width: raw.circle_stroke_width,
            line_color: parse_color("line-color", raw.line_color)?,
            line_opacity: raw.line_opacity,
            line_width: raw.line_width,
            line_cap: raw.line_cap.map(LineCap::from),
            line_join: raw.line_join.map(LineJoin::from),
            fill_color: parse_color("fill-color", raw.fill_color)?,
            fill_opacity: raw.fill_opacity,
            fill_outline_color: parse_color("fill-outline-color", raw.fill_outline_color)?,
        })
    }
}

/// Parse an optional CSS color string into a [`Color`]. `None` stays `None`;
/// an unparseable string is an error naming the canonical property.
fn parse_color(prop: &str, raw: Option<String>) -> Result<Option<Color>, String> {
    raw.map(|s| {
        csscolorparser::parse(&s)
            // csscolorparser yields straight RGBA already clamped to 0..=1.
            .map(|c| Color {
                r: c.r,
                g: c.g,
                b: c.b,
                a: c.a,
            })
            .map_err(|e| format!("invalid CSS color for {prop:?}: {s:?} ({e})"))
    })
    .transpose()
}

#[cfg(test)]
#[allow(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned Value built from json!() macro"
)]
mod tests {
    use martin_core::overlay::{OverlayProperties, OverlaySpec};
    use rstest::rstest;
    use serde_json::{Value, json};

    use super::parse_overlay;

    fn parse(body: Value) -> Result<OverlaySpec, String> {
        parse_overlay(&serde_json::to_vec(&body).expect("serialize body"))
    }

    fn fc(features: Value) -> Value {
        json!({ "type": "FeatureCollection", "features": features })
    }

    fn point(properties: Value) -> Value {
        json!({
            "type": "Feature",
            "geometry": { "type": "Point", "coordinates": [0.0, 0.0] },
            "properties": properties,
        })
    }

    /// Resolve a single point feature's `properties` into its validated style.
    /// Property parsing is geometry-agnostic, so every style test goes through a
    /// point regardless of which geometry the properties would target.
    #[track_caller]
    fn style(properties: Value) -> OverlayProperties {
        let spec = parse(fc(json!([point(properties)]))).expect("parses");
        assert_eq!(spec.features.len(), 1, "expected exactly one feature");
        spec.features[0].properties.clone().unwrap_or_default()
    }

    #[test]
    fn no_properties_leaves_every_field_unset() {
        // Paint defaults are applied at render time, not in the IR -- a bare
        // feature carries no style at all.
        insta::assert_debug_snapshot!(style(json!({})), @"
        OverlayProperties {
            circle_color: None,
            circle_opacity: None,
            circle_radius: None,
            circle_stroke_color: None,
            circle_stroke_opacity: None,
            circle_stroke_width: None,
            line_color: None,
            line_opacity: None,
            line_width: None,
            line_cap: None,
            line_join: None,
            fill_color: None,
            fill_opacity: None,
            fill_outline_color: None,
        }
        ");
    }

    #[test]
    fn canonical_properties_parse_to_typed_style() {
        // Canonical MapLibre names map 1:1: colors become straight RGBA, enums
        // become the core types, numbers pass through.
        insta::assert_debug_snapshot!(style(json!({
            "circle-color": "#ff0000",
            "circle-opacity": 0.5,
            "circle-radius": 8.0,
            "circle-stroke-color": "#fff",
            "circle-stroke-opacity": 0.25,
            "circle-stroke-width": 2.0,
            "line-color": "rgb(0,255,0)",
            "line-opacity": 0.75,
            "line-width": 5.0,
            "line-cap": "round",
            "line-join": "miter",
            "fill-color": "blue",
            "fill-opacity": 1.0,
            "fill-outline-color": "#000000",
        })), @"
        OverlayProperties {
            circle_color: Some(
                Color {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            ),
            circle_opacity: Some(
                0.5,
            ),
            circle_radius: Some(
                8.0,
            ),
            circle_stroke_color: Some(
                Color {
                    r: 1.0,
                    g: 1.0,
                    b: 1.0,
                    a: 1.0,
                },
            ),
            circle_stroke_opacity: Some(
                0.25,
            ),
            circle_stroke_width: Some(
                2.0,
            ),
            line_color: Some(
                Color {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                },
            ),
            line_opacity: Some(
                0.75,
            ),
            line_width: Some(
                5.0,
            ),
            line_cap: Some(
                Round,
            ),
            line_join: Some(
                Miter,
            ),
            fill_color: Some(
                Color {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                    a: 1.0,
                },
            ),
            fill_opacity: Some(
                1.0,
            ),
            fill_outline_color: Some(
                Color {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            ),
        }
        ");
    }

    #[test]
    fn unknown_keys_are_dropped() {
        // id/name/title/description and arbitrary keys are not styling -- they
        // must neither error nor leak into the parsed style.
        insta::assert_debug_snapshot!(style(json!({
            "id": 42,
            "name": "Antarctica HQ",
            "title": "Origin",
            "description": "Where the streams cross",
            "foo": { "bar": "baz" },
            "circle-color": "red",
        })), @"
        OverlayProperties {
            circle_color: Some(
                Color {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                    a: 1.0,
                },
            ),
            circle_opacity: None,
            circle_radius: None,
            circle_stroke_color: None,
            circle_stroke_opacity: None,
            circle_stroke_width: None,
            line_color: None,
            line_opacity: None,
            line_width: None,
            line_cap: None,
            line_join: None,
            fill_color: None,
            fill_opacity: None,
            fill_outline_color: None,
        }
        ");
    }

    #[test]
    fn out_of_range_numbers_are_kept_unvalidated() {
        let style = style(json!({ "circle-opacity": 7.0, "circle-radius": -3.0 }));
        assert_eq!(style.circle_opacity, Some(7.0), "no range check");
        assert_eq!(style.circle_radius, Some(-3.0), "no range check");
    }

    #[test]
    fn null_property_is_treated_as_absent() {
        // A present `null` is leniently mapped onto the Option default rather
        // than rejected.
        assert_eq!(style(json!({ "circle-radius": null })).circle_radius, None);
    }

    #[rstest]
    #[case::color_value(fc(json!([point(json!({ "circle-color": "rebeccapurpel" }))])), "circle-color")]
    #[case::line_cap_value(fc(json!([point(json!({ "line-cap": "diagonal" }))])), "butt")]
    #[case::line_cap_wrong_case(fc(json!([point(json!({ "line-cap": "BUTT" }))])), "butt")]
    #[case::radius_string(fc(json!([point(json!({ "circle-radius": "5" }))])), "f32")]
    #[case::radius_bool(fc(json!([point(json!({ "circle-radius": true }))])), "f32")]
    // A bare Feature and a garbage `type` are valid JSON but not the envelope
    // this endpoint accepts.
    #[case::bare_feature(point(json!({})), "FeatureCollection")]
    #[case::wrong_envelope_type(json!({ "type": "Wibble" }), "FeatureCollection")]
    fn rejects_invalid_input(#[case] body: Value, #[case] expected_fragment: &str) {
        let err = parse(body).expect_err("rejected");
        assert!(
            err.contains(expected_fragment),
            "error {err:?} should mention {expected_fragment:?}"
        );
    }

    #[test]
    fn empty_feature_collection_parses_to_empty_spec() {
        assert!(parse(fc(json!([]))).expect("parses").is_empty());
    }

    #[test]
    fn features_keep_null_geometry_for_apply_time_skipping() {
        // Null/unsupported geometries stay in the IR (skipped later, at apply
        // time) rather than being dropped during parsing.
        let spec = parse(fc(json!([
            { "type": "Feature", "geometry": null, "properties": { "circle-color": "red" } },
            point(json!({ "circle-color": "blue" })),
        ])))
        .expect("parses");
        assert_eq!(spec.features.len(), 2, "both features retained");
        assert!(
            spec.features[0].geometry.is_none(),
            "null geometry kept as None"
        );
        assert!(spec.features[1].geometry.is_some());
    }
}
