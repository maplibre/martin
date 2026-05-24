//! Static-render overlays: parse a maplibre-style-spec subset, then apply it
//! as ephemeral sources+layers on a renderer's style.

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod apply;
mod parse;

#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use apply::{AppliedOverlay, ApplyError, apply_to_style};
pub use parse::{OverlayParseError, parse_spec};

/// Prefix prepended to every caller-supplied source/layer id before it reaches
/// maplibre. Guarantees overlay ids cannot collide with the base style.
const ID_PREFIX: &str = "overlay:";

/// A parsed overlay spec: sources + layers in a maplibre-style-spec subset.
#[derive(Debug, Default, Clone)]
pub struct OverlaySpec {
    /// `GeoJSON` sources, in input order. Source ids are un-prefixed.
    pub sources: Vec<OverlaySource>,
    /// Layers, in declared render order. Layer ids are un-prefixed.
    pub layers: Vec<OverlayLayer>,
}

impl OverlaySpec {
    /// `true` when there are no sources or layers — nothing to apply.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty() && self.layers.is_empty()
    }
}

/// A `GeoJSON` source bound for a maplibre `GeoJsonSource`.
#[derive(Debug, Clone)]
pub struct OverlaySource {
    /// Caller-supplied source id (un-prefixed).
    pub id: String,
    /// Parsed `GeoJSON` payload. Parsed eagerly so malformed data → 400.
    pub data: geojson::GeoJson,
}

/// A single overlay layer. Vec position is the render order.
#[derive(Debug, Clone)]
pub enum OverlayLayer {
    /// Polygon fill layer.
    Fill {
        /// Caller-supplied layer id (un-prefixed).
        id: String,
        /// Caller-supplied source id this layer reads from (un-prefixed).
        source: String,
        /// Base-style layer id to insert before. Passed verbatim to maplibre
        /// — must reference a layer in the base style, never an overlay layer.
        before: Option<String>,
        /// Paint properties.
        paint: FillPaint,
    },
    /// Line layer.
    Line {
        /// Caller-supplied layer id (un-prefixed).
        id: String,
        /// Caller-supplied source id this layer reads from (un-prefixed).
        source: String,
        /// Base-style layer id to insert before. Passed verbatim to maplibre.
        before: Option<String>,
        /// Paint properties.
        paint: LinePaint,
        /// Layout properties.
        layout: LineLayout,
    },
    /// Circle layer (used for markers).
    Circle {
        /// Caller-supplied layer id (un-prefixed).
        id: String,
        /// Caller-supplied source id this layer reads from (un-prefixed).
        source: String,
        /// Base-style layer id to insert before. Passed verbatim to maplibre.
        before: Option<String>,
        /// Paint properties.
        paint: CirclePaint,
    },
}

/// Paint properties for a `fill` layer.
#[derive(Debug, Default, Clone, Copy)]
pub struct FillPaint {
    /// `fill-color`.
    pub color: Option<Color>,
    /// `fill-opacity` in `0..=1`.
    pub opacity: Option<f32>,
    /// `fill-outline-color`.
    pub outline_color: Option<Color>,
}

/// Paint properties for a `line` layer.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinePaint {
    /// `line-color`.
    pub color: Option<Color>,
    /// `line-opacity` in `0..=1`.
    pub opacity: Option<f32>,
    /// `line-width` in pixels at the rendered scale.
    pub width: Option<f32>,
}

/// Layout properties for a `line` layer.
#[derive(Debug, Default, Clone, Copy)]
pub struct LineLayout {
    /// `line-cap`.
    pub cap: Option<LineCap>,
    /// `line-join`.
    pub join: Option<LineJoin>,
}

/// Paint properties for a `circle` layer.
#[derive(Debug, Default, Clone, Copy)]
pub struct CirclePaint {
    /// `circle-color`.
    pub color: Option<Color>,
    /// `circle-opacity` in `0..=1`.
    pub opacity: Option<f32>,
    /// `circle-radius` in pixels at the rendered scale.
    pub radius: Option<f32>,
    /// `circle-stroke-color`.
    pub stroke_color: Option<Color>,
    /// `circle-stroke-opacity` in `0..=1`.
    pub stroke_opacity: Option<f32>,
    /// `circle-stroke-width` in pixels at the rendered scale.
    pub stroke_width: Option<f32>,
}

/// Straight RGBA in `0..=1`, parsed from a CSS color string.
///
/// Owned by this module so the parser doesn't need a maplibre dep.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    /// Square cap that ends flush with the end of the line.
    Butt,
    /// Rounded cap centred at the end of the line.
    Round,
    /// Square cap centred at the end of the line, extending past by half line-width.
    Square,
}

/// `line-join` layout value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    /// Sharp join.
    Miter,
    /// Beveled join.
    Bevel,
    /// Rounded join.
    Round,
}
