//! Wire format for static-render overlays.
//!
//! A simplestyle-shaped `GeoJSON` `FeatureCollection` arrives on the request
//! body and is deserialized here into martin-core's typed [`OverlaySpec`].
//! Deserialization is an application concern, so all of it lives in this crate,
//! not in martin-core — the core only ever sees the already-validated IR.
//!
//! serde validates the envelope, the enums, and the numbers;
//! [`RawOverlayProperties`] then folds the simplestyle aliases into the
//! canonical [`OverlayProperties`] fields (canonical name wins on conflict) and
//! parses the CSS color strings. A bad value surfaces as an error string,
//! mapped to a 400 at the HTTP boundary.

// Compiled standalone under the maplibre-free `overlay` feature so the parser
// and its tests build without maplibre. The only non-test caller is
// `styles_static` (rendering + linux), so dead-code is only meaningful — and
// thus only enforced — in that configuration.
#![cfg_attr(
    not(all(feature = "rendering", target_os = "linux")),
    allow(dead_code)
)]

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
            properties: raw.properties.map(OverlayProperties::try_from).transpose()?,
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
            circle_radius: raw.circle_radius.or_else(|| raw.marker_size.map(MarkerSize::radius)),
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
    use martin_core::overlay::{LineCap, LineJoin, OverlayProperties, OverlaySpec};
    use rstest::rstest;
    use serde_json::{Value, json};

    use super::parse_overlay;

    fn parse(value: Value) -> Result<OverlaySpec, String> {
        parse_overlay(&serde_json::to_vec(&value).expect("serialize value"))
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
        let spec =
            parse(fc(json!([point(json!({ "marker-color": "#ff0000" }))]))).expect("parses");
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
        assert!(err.contains("small"), "names valid set: {err}");
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
        assert!(err.contains("circle-color"), "got {err}");
    }

    #[rstest]
    #[case::diagonal("diagonal")]
    #[case::wrong_case("BUTT")]
    fn invalid_line_cap_rejected(#[case] cap: &str) {
        let err = parse(fc(json!([linestring(json!({ "line-cap": cap }))]))).expect_err("rejects");
        assert!(err.contains("butt"), "names valid set: {err}");
    }

    #[rstest]
    #[case::string(json!("5"))]
    #[case::boolean(json!(true))]
    fn non_numeric_radius_rejected(#[case] value: Value) {
        let err =
            parse(fc(json!([point(json!({ "circle-radius": value }))]))).expect_err("rejects");
        assert!(err.contains("f32"), "expects a number: {err}");
    }

    #[test]
    fn null_number_treated_as_absent() {
        // A present `null` is leniently treated as "unset" rather than a hard
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
            err.to_lowercase().contains("featurecollection") || err.contains("type"),
            "got {err}"
        );
    }

    #[test]
    fn malformed_body_rejected() {
        let err = parse(json!({ "type": "Wibble" })).expect_err("rejects");
        assert!(!err.is_empty(), "got an error");
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
}
