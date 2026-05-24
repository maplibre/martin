//! `FeatureCollection` → typed [`OverlaySpec`].
//!
//! Each feature's `properties` is normalized so simplestyle aliases
//! (`marker-color`, `stroke`, `fill`, etc.) become canonical `MapLibre`
//! property names. Every feature then fans out into one [`OverlaySource`]
//! (containing just that feature) plus 1 or 2 [`OverlayLayer`]s with
//! literal paint values built from the normalized properties.

use csscolorparser::ParseColorError;
use geojson::{GeoJson, GeometryValue, JsonObject};
use serde_json::Value;

use crate::overlay::{
    CirclePaint, Color, FillPaint, LineCap, LineJoin, LineLayout, LinePaint, OverlayFeature,
    OverlaySpec, StyledLayer,
};

/// Errors produced while parsing an [`OverlaySpec`] from JSON.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum OverlayParseError {
    /// Body did not deserialize as a `GeoJSON` object at all.
    #[error("body is not a valid GeoJSON object: {source}")]
    MalformedGeoJson {
        /// Underlying deserialization error.
        #[source]
        source: serde_json::Error,
    },

    /// Body deserialized as a non-`FeatureCollection` `GeoJSON` variant.
    #[error("body must be a FeatureCollection (got a {actual})")]
    NotFeatureCollection {
        /// The `GeoJSON` top-level type that was received.
        actual: &'static str,
    },

    /// A color property's value was not a string, or did not parse as a CSS color.
    #[error("feature {index}: invalid CSS color for {prop:?}: {value:?} ({source})")]
    InvalidColor {
        /// Zero-based index of the offending feature.
        index: usize,
        /// The canonical property name (post alias-normalization).
        prop: &'static str,
        /// Echoed-back raw value for diagnostics.
        value: String,
        /// Underlying `csscolorparser` error.
        #[source]
        source: ParseColorError,
    },

    /// A numeric property was not a JSON number.
    #[error("feature {index}: invalid number for {prop:?}")]
    InvalidNumber {
        /// Zero-based index of the offending feature.
        index: usize,
        /// The canonical property name.
        prop: &'static str,
    },

    /// `line-cap` was set but not one of `butt`, `round`, `square`.
    #[error("feature {index}: invalid line-cap; expected one of: butt, round, square")]
    InvalidLineCap {
        /// Zero-based index of the offending feature.
        index: usize,
    },

    /// `line-join` was set but not one of `miter`, `bevel`, `round`.
    #[error("feature {index}: invalid line-join; expected one of: miter, bevel, round")]
    InvalidLineJoin {
        /// Zero-based index of the offending feature.
        index: usize,
    },

    /// `marker-size` was set but not one of `small`, `medium`, `large`.
    #[error("feature {index}: invalid marker-size; expected one of: small, medium, large")]
    InvalidMarkerSize {
        /// Zero-based index of the offending feature.
        index: usize,
    },
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

const DEFAULT_CIRCLE_COLOR: Color = rgb(0x7e, 0x7e, 0x7e);
const DEFAULT_LINE_COLOR: Color = rgb(0x55, 0x55, 0x55);
const DEFAULT_FILL_COLOR: Color = rgb(0x55, 0x55, 0x55);
const DEFAULT_CIRCLE_RADIUS: f32 = 8.0;
const DEFAULT_LINE_WIDTH: f32 = 2.0;
const DEFAULT_FILL_OPACITY: f32 = 0.6;
const DEFAULT_OPACITY: f32 = 1.0;

const FILL_KEYS: &[&str] = &["fill-color", "fill-opacity", "fill-outline-color"];
const LINE_KEYS: &[&str] = &[
    "line-color",
    "line-opacity",
    "line-width",
    "line-cap",
    "line-join",
];

/// Simplestyle → canonical 1:1 property-name aliases. `marker-size` is
/// handled separately because it requires enum-to-numeric translation.
const ALIASES: &[(&str, &str)] = &[
    ("marker-color", "circle-color"),
    ("stroke", "line-color"),
    ("stroke-opacity", "line-opacity"),
    ("stroke-width", "line-width"),
    ("fill", "fill-color"),
];

/// Parse a JSON tree (a `GeoJSON` `FeatureCollection`) into an [`OverlaySpec`].
///
/// Each input feature becomes one [`OverlayFeature`] carrying its own
/// `GeoJSON` data and 1 or 2 [`StyledLayer`]s — picked by geometry type and
/// which style properties the feature carries. Simplestyle aliases on
/// `feature.properties` (e.g. `marker-color`, `stroke`, `fill`) are
/// normalized to canonical `MapLibre` names before paint values are
/// extracted; on conflict, the canonical name wins. Features with an
/// unsupported or `null` geometry are skipped.
///
/// # Errors
///
/// Returns [`OverlayParseError`] when the body is not a valid `GeoJSON`
/// `FeatureCollection`, or when a styling property has the wrong type
/// (non-string colors, non-numeric widths, unknown enum values). Property
/// keys that don't map to any known paint/layout property are silently
/// ignored — including the simplestyle `title` and `description` fields.
pub fn parse_spec(body: &Value) -> Result<OverlaySpec, OverlayParseError> {
    let gj: GeoJson = serde_json::from_value(body.clone())
        .map_err(|source| OverlayParseError::MalformedGeoJson { source })?;
    let fc = match gj {
        GeoJson::FeatureCollection(fc) => fc,
        GeoJson::Feature(_) => {
            return Err(OverlayParseError::NotFeatureCollection { actual: "Feature" });
        }
        GeoJson::Geometry(_) => {
            return Err(OverlayParseError::NotFeatureCollection { actual: "Geometry" });
        }
    };

    let mut features = Vec::with_capacity(fc.features.len());
    for (index, mut feature) in fc.features.into_iter().enumerate() {
        let Some(geometry) = feature.geometry.as_ref() else {
            continue;
        };
        let Some(kind) = supported_geometry_kind(&geometry.value) else {
            continue;
        };
        let props = normalize_properties(index, feature.properties.take())?;
        let layers = match kind {
            SupportedKind::Point => vec![StyledLayer::Circle(build_circle_paint(index, &props)?)],
            SupportedKind::Line => vec![StyledLayer::Line(
                build_line_paint(index, &props)?,
                build_line_layout(index, &props)?,
            )],
            SupportedKind::Polygon => polygon_layers(index, &props)?,
        };

        feature.properties = Some(props);
        features.push(OverlayFeature {
            data: GeoJson::Feature(feature),
            layers,
        });
    }

    Ok(OverlaySpec { features })
}

#[derive(Copy, Clone)]
enum SupportedKind {
    Point,
    Line,
    Polygon,
}

fn supported_geometry_kind(value: &GeometryValue) -> Option<SupportedKind> {
    match value {
        GeometryValue::Point { .. } | GeometryValue::MultiPoint { .. } => {
            Some(SupportedKind::Point)
        }
        GeometryValue::LineString { .. } | GeometryValue::MultiLineString { .. } => {
            Some(SupportedKind::Line)
        }
        GeometryValue::Polygon { .. } | GeometryValue::MultiPolygon { .. } => {
            Some(SupportedKind::Polygon)
        }
        GeometryValue::GeometryCollection { .. } => None,
    }
}

fn normalize_properties(
    index: usize,
    props: Option<JsonObject>,
) -> Result<JsonObject, OverlayParseError> {
    let mut props = props.unwrap_or_default();
    for (alias, canonical) in ALIASES {
        if let Some(value) = props.remove(*alias) {
            props.entry((*canonical).to_string()).or_insert(value);
        }
    }
    if let Some(value) = props.remove("marker-size")
        && !props.contains_key("circle-radius")
    {
        let Some(s) = value.as_str() else {
            return Err(OverlayParseError::InvalidMarkerSize { index });
        };
        let radius: f64 = match s {
            "small" => 6.0,
            "medium" => 8.0,
            "large" => 10.0,
            _ => return Err(OverlayParseError::InvalidMarkerSize { index }),
        };
        props.insert(
            "circle-radius".to_string(),
            Value::Number(
                serde_json::Number::from_f64(radius)
                    .expect("hard-coded finite literal is a valid serde_json::Number"),
            ),
        );
    }
    Ok(props)
}

/// A polygon emits a fill layer (unless only stroke properties are set), and
/// a line layer when any stroke/line property is present. A bare polygon
/// still fills so it stays visible.
fn polygon_layers(index: usize, props: &JsonObject) -> Result<Vec<StyledLayer>, OverlayParseError> {
    let has_fill_prop = FILL_KEYS.iter().any(|k| props.contains_key(*k));
    let has_line_prop = LINE_KEYS.iter().any(|k| props.contains_key(*k));
    let mut out = Vec::with_capacity(2);
    if has_fill_prop || !has_line_prop {
        out.push(StyledLayer::Fill(build_fill_paint(index, props)?));
    }
    if has_line_prop {
        out.push(StyledLayer::Line(
            build_line_paint(index, props)?,
            build_line_layout(index, props)?,
        ));
    }
    Ok(out)
}

fn build_circle_paint(index: usize, props: &JsonObject) -> Result<CirclePaint, OverlayParseError> {
    Ok(CirclePaint {
        color: Some(get_color(index, props, "circle-color")?.unwrap_or(DEFAULT_CIRCLE_COLOR)),
        opacity: Some(get_number(index, props, "circle-opacity")?.unwrap_or(DEFAULT_OPACITY)),
        radius: Some(get_number(index, props, "circle-radius")?.unwrap_or(DEFAULT_CIRCLE_RADIUS)),
        stroke_color: get_color(index, props, "circle-stroke-color")?,
        stroke_opacity: get_number(index, props, "circle-stroke-opacity")?,
        stroke_width: get_number(index, props, "circle-stroke-width")?,
    })
}

fn build_line_paint(index: usize, props: &JsonObject) -> Result<LinePaint, OverlayParseError> {
    Ok(LinePaint {
        color: Some(get_color(index, props, "line-color")?.unwrap_or(DEFAULT_LINE_COLOR)),
        opacity: Some(get_number(index, props, "line-opacity")?.unwrap_or(DEFAULT_OPACITY)),
        width: Some(get_number(index, props, "line-width")?.unwrap_or(DEFAULT_LINE_WIDTH)),
    })
}

fn build_line_layout(index: usize, props: &JsonObject) -> Result<LineLayout, OverlayParseError> {
    let cap = match props.get("line-cap") {
        None => None,
        Some(Value::String(s)) => Some(match s.as_str() {
            "butt" => LineCap::Butt,
            "round" => LineCap::Round,
            "square" => LineCap::Square,
            _ => return Err(OverlayParseError::InvalidLineCap { index }),
        }),
        Some(_) => return Err(OverlayParseError::InvalidLineCap { index }),
    };
    let join = match props.get("line-join") {
        None => None,
        Some(Value::String(s)) => Some(match s.as_str() {
            "miter" => LineJoin::Miter,
            "bevel" => LineJoin::Bevel,
            "round" => LineJoin::Round,
            _ => return Err(OverlayParseError::InvalidLineJoin { index }),
        }),
        Some(_) => return Err(OverlayParseError::InvalidLineJoin { index }),
    };
    Ok(LineLayout { cap, join })
}

fn build_fill_paint(index: usize, props: &JsonObject) -> Result<FillPaint, OverlayParseError> {
    Ok(FillPaint {
        color: Some(get_color(index, props, "fill-color")?.unwrap_or(DEFAULT_FILL_COLOR)),
        opacity: Some(get_number(index, props, "fill-opacity")?.unwrap_or(DEFAULT_FILL_OPACITY)),
        outline_color: get_color(index, props, "fill-outline-color")?,
    })
}

fn get_color(
    index: usize,
    props: &JsonObject,
    prop: &'static str,
) -> Result<Option<Color>, OverlayParseError> {
    let Some(value) = props.get(prop) else {
        return Ok(None);
    };
    let Value::String(s) = value else {
        return Err(OverlayParseError::InvalidColor {
            index,
            prop,
            value: value.to_string(),
            source: csscolorparser::parse("")
                .expect_err("empty string never parses as a CSS color"),
        });
    };
    let parsed = csscolorparser::parse(s).map_err(|source| OverlayParseError::InvalidColor {
        index,
        prop,
        value: s.clone(),
        source,
    })?;
    Ok(Some(Color {
        r: parsed.r as f32,
        g: parsed.g as f32,
        b: parsed.b as f32,
        a: parsed.a as f32,
    }))
}

fn get_number(
    index: usize,
    props: &JsonObject,
    prop: &'static str,
) -> Result<Option<f32>, OverlayParseError> {
    let Some(value) = props.get(prop) else {
        return Ok(None);
    };
    if value.is_null() {
        return Err(OverlayParseError::InvalidNumber { index, prop });
    }
    let n = value
        .as_f64()
        .ok_or(OverlayParseError::InvalidNumber { index, prop })?;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "downcast to f32 for the maplibre paint API"
    )]
    Ok(Some(n as f32))
}
