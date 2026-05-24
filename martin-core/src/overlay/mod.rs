//! Static-render overlays: parse a simplestyle-shaped `FeatureCollection`
//! (with `MapLibre` property names on each feature's `properties`) into a
//! list of pre-styled features, then apply them ephemerally to a renderer's
//! style.

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod apply;
mod parse;

#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use apply::{AppliedOverlay, ApplyError, apply_to_style};
pub use parse::{OverlayParseError, parse_spec};

/// Prefix prepended to every caller-supplied source/layer id before it reaches
/// maplibre. Guarantees overlay ids cannot collide with the base style.
#[cfg(all(feature = "rendering", target_os = "linux"))]
const ID_PREFIX: &str = "overlay:";

/// A parsed overlay: an ordered list of features, each pre-styled with the
/// one or two layers it renders as.
#[derive(Debug, Default, Clone)]
pub struct OverlaySpec {
    /// Features in render order. Each feature renders independently as its
    /// own `GeoJSON` source plus 1 or 2 typed layers.
    pub features: Vec<OverlayFeature>,
}

impl OverlaySpec {
    /// `true` when there are no features — nothing to apply.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.features.is_empty()
    }
}

/// One feature and the 1-2 layers it draws as. The styled layers are
/// pre-resolved at parse time — there are no cross-references to other
/// features and no string-id pointers.
#[derive(Debug, Clone)]
pub struct OverlayFeature {
    /// `GeoJSON` payload (typically a single-feature `Feature`) that becomes
    /// this overlay's source. Parsed eagerly so malformed data → 400.
    pub data: geojson::GeoJson,
    /// Layers in draw order (fill → outline-line → line → circle).
    /// A point yields exactly `Circle`; a linestring yields exactly `Line`;
    /// a polygon yields `Fill`, `Line`, or both.
    pub layers: Vec<StyledLayer>,
}

/// One typed paint layer attached to a feature.
#[derive(Debug, Clone, Copy)]
pub enum StyledLayer {
    /// Polygon fill paint.
    Fill(FillPaint),
    /// Line paint + layout (used for `LineString`/`MultiLineString` features
    /// and for polygon outlines).
    Line(LinePaint, LineLayout),
    /// Circle paint (used for `Point`/`MultiPoint` features).
    Circle(CirclePaint),
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
