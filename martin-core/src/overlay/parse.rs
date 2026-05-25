//! Deserialization support for the overlay boundary IR.
//!
//! [`RawOverlayProperties`] is the wire shape of a feature's `properties`: it
//! carries both canonical `MapLibre` names and the simplestyle aliases as
//! separate fields, lets serde validate every value (colors as strings, enums,
//! numbers), then [`TryFrom`] folds the aliases into the canonical
//! [`OverlayProperties`] (canonical wins) and parses the CSS colors. A bad
//! value surfaces as a deserialization error.

use serde::Deserialize;

use crate::overlay::{Color, LineCap, LineJoin, OverlayProperties};

/// `"FeatureCollection"` discriminator for the top-level body. A unit enum so
/// any other `type` (a bare `Feature`, `Geometry`, or garbage) fails to
/// deserialize with a clear serde error.
#[derive(Debug, Default, Clone, Deserialize)]
pub(crate) enum FeatureCollectionTag {
    #[default]
    FeatureCollection,
}

/// `marker-size` simplestyle enum; translated to a `circle-radius` value.
#[derive(Debug, Clone, Copy, Deserialize)]
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

/// Wire shape of a feature's `properties`. Colors arrive as strings and are
/// parsed in [`TryFrom`]; enums and numbers are validated by serde. Unknown
/// keys (e.g. simplestyle `title`/`description`) are ignored.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub(crate) struct RawOverlayProperties {
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
    line_cap: Option<LineCap>,
    line_join: Option<LineJoin>,
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
            line_cap: raw.line_cap,
            line_join: raw.line_join,
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
            .map(Color::from)
            .map_err(|e| format!("invalid CSS color for {prop:?}: {s:?} ({e})"))
    })
    .transpose()
}

impl From<csscolorparser::Color> for Color {
    fn from(c: csscolorparser::Color) -> Self {
        // csscolorparser yields straight RGBA already clamped to 0..=1.
        Self {
            r: c.r,
            g: c.g,
            b: c.b,
            a: c.a,
        }
    }
}
