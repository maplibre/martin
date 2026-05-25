//! Static-render overlays: the typed boundary IR a rendered base map is
//! decorated with.
//!
//! [`OverlaySpec`] is a pre-validated `GeoJSON` `FeatureCollection`. The wire
//! format -- the CSS-color strings, the `FeatureCollection` envelope -- is an
//! application concern, so the `martin` crate owns deserialization and builds
//! these types from a request body. martin-core only ever sees the already-valid
//! IR. The geometry→layer fan-out is a rendering concern and lives in [`apply`];
//! any paint property left unset there falls through to `MapLibre`'s own defaults.

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod apply;

#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use apply::{AppliedOverlay, ApplyError, apply_to_style};

/// Prefix prepended to every synthetic source/layer id before it reaches
/// maplibre. Guarantees overlay ids cannot collide with the base style.
#[cfg(all(feature = "rendering", target_os = "linux"))]
const ID_PREFIX: &str = "overlay:";

/// Boundary IR: a `GeoJSON` `FeatureCollection` of pre-validated overlay
/// features. Built by the application layer from the request body; by the time
/// it reaches martin-core every value is already valid.
#[derive(Debug, Default, Clone)]
pub struct OverlaySpec {
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
#[derive(Debug, Clone)]
pub struct OverlayFeature {
    /// Geometry to render. `Point`/`MultiPoint` → circle; `LineString`/
    /// `MultiLineString` → line; `Polygon`/`MultiPolygon` → fill and/or line.
    pub geometry: Option<geojson::Geometry>,
    /// Validated style for this feature; `None` is treated as empty.
    pub properties: Option<OverlayProperties>,
}

/// Per-feature style, keyed by canonical `MapLibre` paint/layout names. The
/// application layer builds this from the wire format, dropping unknown keys
/// (`title`, `id`, …). All fields are optional; an unset field falls through to
/// `MapLibre`'s own paint default at render time, in [`apply`].
#[derive(Debug, Default, Clone)]
pub struct OverlayProperties {
    /// `circle-color`.
    pub circle_color: Option<Color>,
    /// `circle-opacity`.
    pub circle_opacity: Option<f32>,
    /// `circle-radius`.
    pub circle_radius: Option<f32>,
    /// `circle-stroke-color`.
    pub circle_stroke_color: Option<Color>,
    /// `circle-stroke-opacity`.
    pub circle_stroke_opacity: Option<f32>,
    /// `circle-stroke-width`.
    pub circle_stroke_width: Option<f32>,
    /// `line-color`.
    pub line_color: Option<Color>,
    /// `line-opacity`.
    pub line_opacity: Option<f32>,
    /// `line-width`.
    pub line_width: Option<f32>,
    /// `line-cap`.
    pub line_cap: Option<LineCap>,
    /// `line-join`.
    pub line_join: Option<LineJoin>,
    /// `fill-color`.
    pub fill_color: Option<Color>,
    /// `fill-opacity`.
    pub fill_opacity: Option<f32>,
    /// `fill-outline-color`.
    pub fill_outline_color: Option<Color>,
}

/// Straight RGBA in `0..=1`.
///
/// Owned by this module so the maplibre-free `overlay` feature doesn't need a
/// maplibre dependency; [`apply`] converts it via `From` at render time. The
/// application layer constructs it directly from a parsed CSS color.
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
