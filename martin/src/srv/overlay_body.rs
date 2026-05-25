//! Wire format for static-render overlays.
//!
//! A simplestyle-shaped `GeoJSON` `FeatureCollection` arrives on the request
//! body and is deserialized here into martin-core's typed [`OverlaySpec`].
//! Deserialization is an application concern, so all of it lives in this crate,
//! not in martin-core - the core only ever sees the already-validated IR.
//!
//! serde validates the envelope, the enums, and the numbers;
//! [`RawOverlayProperties`] then folds the simplestyle aliases into the
//! canonical [`OverlayProperties`] fields (canonical name wins on conflict) and
//! parses the CSS color strings. A bad value surfaces as an error string,
//! mapped to a 400 at the HTTP boundary.

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
    let raw: RawFeatureCollection =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid JSON overlay body: {e}"))?;
    OverlaySpec::try_from(raw)
}

/// `"FeatureCollection"` discriminator for the top-level body. A unit enum so
/// any other `type` (a bare `Feature`, `Geometry`, or garbage) fails to
/// deserialize with a clear serde error.
#[derive(Deserialize)]
enum FeatureCollectionTag {
    FeatureCollection,
}

/// Wire shape of the top-level body.
#[derive(Deserialize)]
struct RawFeatureCollection {
    #[serde(rename = "type")]
    #[expect(dead_code, reason = "validated by Deserialize, then discarded")]
    tag: FeatureCollectionTag,
    features: Vec<RawFeature>,
}

impl TryFrom<RawFeatureCollection> for OverlaySpec {
    type Error = String;

    fn try_from(raw: RawFeatureCollection) -> Result<Self, Self::Error> {
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
struct RawFeature {
    #[serde(default)]
    geometry: Option<geojson::Geometry>,
    #[serde(default)]
    properties: Option<RawOverlayProperties>,
}

impl TryFrom<RawFeature> for OverlayFeature {
    type Error = String;

    fn try_from(raw: RawFeature) -> Result<Self, Self::Error> {
        Ok(Self {
            geometry: raw.geometry,
            properties: raw
                .properties
                .map(OverlayProperties::try_from)
                .transpose()?,
        })
    }
}

/// `marker-size` simplestyle enum; translated to a `circle-radius` value.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum MarkerSize {
    Small,
    Medium,
    Large,
}

impl MarkerSize {
    fn radius(self) -> f32 {
        match self {
            Self::Small => 6.0,
            Self::Medium => 8.0,
            Self::Large => 10.0,
        }
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

/// Wire shape of a feature's `properties`. Colors arrive as strings and are
/// parsed in [`TryFrom`]; enums and numbers are validated by serde. Unknown
/// keys (e.g. simplestyle `title`/`description`) are ignored.
#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
struct RawOverlayProperties {
    marker_color: Option<String>,
    circle_color: Option<String>,
    circle_opacity: Option<f32>,
    marker_size: Option<MarkerSize>,
    circle_radius: Option<f32>,
    circle_stroke_color: Option<String>,
    circle_stroke_opacity: Option<f32>,
    circle_stroke_width: Option<f32>,
    stroke: Option<String>,
    line_color: Option<String>,
    stroke_opacity: Option<f32>,
    line_opacity: Option<f32>,
    stroke_width: Option<f32>,
    line_width: Option<f32>,
    line_cap: Option<WireLineCap>,
    line_join: Option<WireLineJoin>,
    fill: Option<String>,
    fill_color: Option<String>,
    fill_opacity: Option<f32>,
    fill_outline_color: Option<String>,
}

impl TryFrom<RawOverlayProperties> for OverlayProperties {
    type Error = String;

    fn try_from(raw: RawOverlayProperties) -> Result<Self, Self::Error> {
        Ok(Self {
            circle_color: parse_color("circle-color", raw.circle_color.or(raw.marker_color))?,
            circle_opacity: raw.circle_opacity,
            circle_radius: raw
                .circle_radius
                .or_else(|| raw.marker_size.map(MarkerSize::radius)),
            circle_stroke_color: parse_color("circle-stroke-color", raw.circle_stroke_color)?,
            circle_stroke_opacity: raw.circle_stroke_opacity,
            circle_stroke_width: raw.circle_stroke_width,
            line_color: parse_color("line-color", raw.line_color.or(raw.stroke))?,
            line_opacity: raw.line_opacity.or(raw.stroke_opacity),
            line_width: raw.line_width.or(raw.stroke_width),
            line_cap: raw.line_cap.map(LineCap::from),
            line_join: raw.line_join.map(LineJoin::from),
            fill_color: parse_color("fill-color", raw.fill_color.or(raw.fill))?,
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
        // Simplestyle defaults are applied at render time, not in the IR — a
        // bare feature carries no style at all.
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
    fn simplestyle_aliases_fold_into_canonical_fields() {
        // marker-*/stroke*/fill are simplestyle aliases for the circle/line/fill
        // fields; marker-size maps to a radius.
        insta::assert_debug_snapshot!(style(json!({
            "marker-color": "#ff0000",
            "marker-size": "large",
            "stroke": "#00ff00",
            "stroke-opacity": 0.25,
            "stroke-width": 5,
            "fill": "blue",
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
            circle_radius: Some(
                10.0,
            ),
            circle_stroke_color: None,
            circle_stroke_opacity: None,
            circle_stroke_width: None,
            line_color: Some(
                Color {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                },
            ),
            line_opacity: Some(
                0.25,
            ),
            line_width: Some(
                5.0,
            ),
            line_cap: None,
            line_join: None,
            fill_color: Some(
                Color {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                    a: 1.0,
                },
            ),
            fill_opacity: None,
            fill_outline_color: None,
        }
        ");
    }

    #[test]
    fn canonical_names_win_over_aliases_on_conflict() {
        // When a feature sets both the canonical name and its alias, the
        // canonical value is kept (here: red/99/green/blue, never black).
        insta::assert_debug_snapshot!(style(json!({
            "marker-color": "#000000", "circle-color": "#ff0000",
            "marker-size": "small",   "circle-radius": 99.0,
            "stroke": "#000000",      "line-color": "#00ff00",
            "fill": "#000000",        "fill-color": "blue",
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
            circle_radius: Some(
                99.0,
            ),
            circle_stroke_color: None,
            circle_stroke_opacity: None,
            circle_stroke_width: None,
            line_color: Some(
                Color {
                    r: 0.0,
                    g: 1.0,
                    b: 0.0,
                    a: 1.0,
                },
            ),
            line_opacity: None,
            line_width: None,
            line_cap: None,
            line_join: None,
            fill_color: Some(
                Color {
                    r: 0.0,
                    g: 0.0,
                    b: 1.0,
                    a: 1.0,
                },
            ),
            fill_opacity: None,
            fill_outline_color: None,
        }
        ");
    }

    #[test]
    fn unknown_keys_are_dropped() {
        // id/name/title/description and arbitrary keys are not styling — they
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

    #[rstest]
    #[case::small("small", 6.0)]
    #[case::medium("medium", 8.0)]
    #[case::large("large", 10.0)]
    fn marker_size_maps_to_circle_radius(#[case] size: &str, #[case] radius: f32) {
        assert_eq!(
            style(json!({ "marker-size": size })).circle_radius,
            Some(radius)
        );
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
    #[case::marker_size_enum(fc(json!([point(json!({ "marker-size": "huge" }))])), "small")]
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
