//! Static-render overlays: a simplestyle-shaped GeoJSON `FeatureCollection`
//! deserializes directly into the typed [`OverlaySpec`] boundary IR. Every bit
//! of validation -- CSS colors, enum values, simplestyle alias resolution --
//! happens during deserialization (see [`parse`]), so the rest of martin-core
//! only ever sees fully-valid input. The geometry→layer fan-out and the
//! simplestyle paint defaults are a rendering concern and live in [`apply`].

use serde::Deserialize;

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod apply;
mod parse;

#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use apply::{AppliedOverlay, ApplyError, apply_to_style};

/// Prefix prepended to every synthetic source/layer id before it reaches
/// maplibre. Guarantees overlay ids cannot collide with the base style.
#[cfg(all(feature = "rendering", target_os = "linux"))]
const ID_PREFIX: &str = "overlay:";

/// Boundary IR: a `GeoJSON` `FeatureCollection` of pre-validated overlay
/// features. Deserializing this type *is* the validation step; a bad body is
/// a deserialization error (→ 400 at the HTTP boundary).
#[derive(Debug, Default, Clone, Deserialize)]
pub struct OverlaySpec {
    /// `"FeatureCollection"` discriminator. Validated on the way in, then
    /// discarded -- a non-`FeatureCollection` body fails to deserialize.
    #[serde(rename = "type")]
    #[expect(dead_code, reason = "validated by Deserialize, then discarded")]
    kind: parse::FeatureCollectionTag,
    /// Features in render order. Each renders independently as its own
    /// `GeoJSON` source plus the 1-2 layers its geometry fans out to.
    pub features: Vec<OverlayFeature>,
}

impl OverlaySpec {
    /// `true` when there are no features -- nothing to apply.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }
}

/// One `GeoJSON` `Feature`: a geometry plus its validated style. The
/// geometry→layer fan-out is deferred to [`apply`], so features with a `null`
/// or unsupported geometry are kept here and skipped at apply time.
#[derive(Debug, Clone, Deserialize)]
pub struct OverlayFeature {
    /// Geometry to render. `Point`/`MultiPoint` → circle; `LineString`/
    /// `MultiLineString` → line; `Polygon`/`MultiPolygon` → fill and/or line.
    #[serde(default)]
    pub geometry: Option<geojson::Geometry>,
    /// Validated style for this feature; `None`/`null` is treated as empty.
    #[serde(default)]
    pub properties: Option<OverlayProperties>,
}

/// Per-feature style, keyed by canonical `MapLibre` paint/layout names. Built by
/// deserialization: simplestyle aliases (`marker-color`, `stroke`, `fill`,
/// `marker-size`) are resolved into these fields (the canonical name wins on
/// conflict), and unknown keys (`title`, `id`, …) are ignored. All fields are
/// optional; the simplestyle defaults are applied later, in [`apply`].
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(try_from = "parse::RawOverlayProperties")]
pub struct OverlayProperties {
    /// `circle-color` (simplestyle alias: `marker-color`).
    pub circle_color: Option<Color>,
    /// `circle-opacity`.
    pub circle_opacity: Option<f32>,
    /// `circle-radius` (simplestyle alias: `marker-size` → 6/8/10).
    pub circle_radius: Option<f32>,
    /// `circle-stroke-color`.
    pub circle_stroke_color: Option<Color>,
    /// `circle-stroke-opacity`.
    pub circle_stroke_opacity: Option<f32>,
    /// `circle-stroke-width`.
    pub circle_stroke_width: Option<f32>,
    /// `line-color` (simplestyle alias: `stroke`).
    pub line_color: Option<Color>,
    /// `line-opacity` (simplestyle alias: `stroke-opacity`).
    pub line_opacity: Option<f32>,
    /// `line-width` (simplestyle alias: `stroke-width`).
    pub line_width: Option<f32>,
    /// `line-cap`.
    pub line_cap: Option<LineCap>,
    /// `line-join`.
    pub line_join: Option<LineJoin>,
    /// `fill-color` (simplestyle alias: `fill`).
    pub fill_color: Option<Color>,
    /// `fill-opacity`.
    pub fill_opacity: Option<f32>,
    /// `fill-outline-color`.
    pub fill_outline_color: Option<Color>,
}

/// Straight RGBA in `0..=1`, parsed from a CSS color string at deserialization.
///
/// Owned by this module so the maplibre-free `overlay` feature doesn't need a
/// maplibre dependency; [`apply`] converts it via `From` at render time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red channel.
    pub r: f32,
    /// Green channel.
    pub g: f32,
    /// Blue channel.
    pub b: f32,
    /// Alpha channel.
    pub a: f32,
}

/// `line-cap` layout value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineCap {
    /// Square cap that ends flush with the end of the line.
    Butt,
    /// Rounded cap centred at the end of the line.
    Round,
    /// Square cap centred at the end of the line, extending past by half line-width.
    Square,
}

/// `line-join` layout value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineJoin {
    /// Sharp join.
    Miter,
    /// Beveled join.
    Bevel,
    /// Rounded join.
    Round,
}
