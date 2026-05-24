//! Draw vector overlays (lines, polygons, point markers) onto a rendered map.
//!
//! Build a [`ParsedOverlays`] from a `GeoJSON` `FeatureCollection` (via
//! [`parse_feature_collection`]) or construct [`Shape`]s and [`Marker`]s
//! directly, then call [`draw_overlays_into`] to composite them onto an
//! [`image::RgbaImage`].
//!
//! The styling vocabulary ([`Rgba`], [`Stroke`], [`Fill`], [`MarkerStyle`])
//! is owned by this crate so callers do not need a direct dependency on the
//! underlying rasterizer.

use geo_types::Coord;

mod draw;
mod parse;
mod project;

pub use draw::{DrawError, draw_overlays_into};
pub use parse::{OverlayParseError, ParsedOverlays, parse_feature_collection};

/// Straight 8-bit RGBA color. Not premultiplied.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgba {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel. `0` is fully transparent, `255` fully opaque.
    pub a: u8,
}

impl Rgba {
    /// Construct an RGBA color with an explicit alpha.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Construct a fully opaque color.
    #[must_use]
    pub const fn opaque(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b, 255)
    }
}

/// Outline style for a line or polygon.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    /// Stroke color.
    pub color: Rgba,
    /// Stroke width in pixels at the rendered scale.
    pub width: f32,
}

impl Stroke {
    /// Default stroke width when none is given (per simplestyle).
    pub const DEFAULT_WIDTH: f32 = 2.0;

    /// Default stroke color when none is given (per simplestyle).
    pub const DEFAULT_COLOR: Rgba = Rgba::opaque(0x55, 0x55, 0x55);

    /// Construct a stroke with the given color and width.
    #[must_use]
    pub const fn new(color: Rgba, width: f32) -> Self {
        Self { color, width }
    }
}

/// Interior style for a polygon.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fill {
    /// Fill color. Alpha applies to the fill area.
    pub color: Rgba,
}

impl Fill {
    /// Default fill alpha when only a CSS color (no opacity) is given (per simplestyle).
    pub const DEFAULT_OPACITY: f32 = 0.6;

    /// Construct a fill with the given color.
    #[must_use]
    pub const fn new(color: Rgba) -> Self {
        Self { color }
    }
}

/// A path overlay: either a polyline (`Line`) or an area (`Polygon`).
///
/// Variants enforce shape invariants: lines always have a stroke and never a
/// fill; polygons may opt out of either stroke or fill but carry holes only
/// when there is an outer ring.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Shape {
    /// A polyline. Points are in WGS84 degrees (`x = lon`, `y = lat`).
    Line {
        /// The polyline vertices, in order.
        points: Vec<Coord>,
        /// Stroke style. Lines without a stroke are invisible, so the stroke is required.
        stroke: Stroke,
    },
    /// A polygon, optionally with holes. Coordinates are in WGS84 degrees.
    Polygon {
        /// Outer ring vertices.
        outer: Vec<Coord>,
        /// Interior rings (holes). Empty when the polygon is hole-free.
        holes: Vec<Vec<Coord>>,
        /// Outline style. `None` means no outline.
        stroke: Option<Stroke>,
        /// Interior style. `None` means no fill (holes are still honoured for the stroke).
        fill: Option<Fill>,
    },
}

/// A point overlay rendered as a filled circle (with optional outline reserved for future use).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Marker {
    /// Marker position in WGS84 degrees (`x = lon`, `y = lat`).
    pub coord: Coord,
    /// Visual style.
    pub style: MarkerStyle,
}

impl Marker {
    /// Construct a marker at `coord` with the default style.
    #[must_use]
    pub fn new(coord: Coord) -> Self {
        Self {
            coord,
            style: MarkerStyle::DEFAULT,
        }
    }
}

/// Visual style for a [`Marker`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MarkerStyle {
    /// Fill color of the marker circle.
    pub color: Rgba,
    /// Radius of the marker circle, in pixels at the rendered scale.
    pub radius: f32,
}

impl MarkerStyle {
    /// Default marker style: red circle, 8px radius (matches Mapbox simplestyle).
    pub const DEFAULT: Self = Self {
        color: Rgba::opaque(255, 0, 0),
        radius: 8.0,
    };
}

/// View parameters describing the camera for an overlay draw call.
#[non_exhaustive]
#[derive(Debug, Clone, Copy)]
pub struct OverlayView {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Camera center in WGS84 degrees (`x = lon`, `y = lat`).
    pub center: Coord,
    /// Map zoom level. Bumping zoom by `log2(pixel_ratio)` aligns overlays
    /// with an `@Nx` base map.
    pub zoom: f64,
}

impl OverlayView {
    /// Construct an [`OverlayView`] for an `image_width × image_height` canvas
    /// centered on `center` at the given `zoom`.
    #[must_use]
    pub const fn new(width: u32, height: u32, center: Coord, zoom: f64) -> Self {
        Self {
            width,
            height,
            center,
            zoom,
        }
    }
}
