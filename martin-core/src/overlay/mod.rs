//! Static images - draw paths/markers via tiny-skia over a map.

use geo_types::Coord;
use tiny_skia::Paint;

mod draw;
mod parse;
mod project;

pub use draw::draw_overlays;
pub use parse::{OverlayParseError, ParsedOverlays, parse_feature_collection};

/// A path overlay (`LineString` or `Polygon`) to draw on the map.
#[derive(Debug, Clone)]
pub struct PathOverlay {
    /// Outer ring for polygons, or the full point sequence for line strings.
    pub points: Vec<Coord>,
    /// Interior rings for polygons; empty for line strings.
    pub holes: Vec<Vec<Coord>>,
    /// Stroke paint, applied to the outline. `None` means no outline.
    pub stroke: Option<Paint<'static>>,
    /// Fill paint, applied to the interior of polygons. `None` means no fill.
    pub fill: Option<Paint<'static>>,
    /// Stroke width in pixels at the rendered scale. `None` falls back to the
    /// drawer's default.
    pub width: Option<f32>,
}

/// A marker overlay to draw on the map as a filled circle.
#[derive(Debug, Clone)]
pub struct MarkerOverlay {
    /// Marker position in WGS84 degrees (`x = lon`, `y = lat`).
    pub coord: Coord,
    /// Tints the circle fill; defaults to red when `None`.
    pub marker_color: Option<Paint<'static>>,
}

/// View parameters describing the camera for an overlay draw call.
#[derive(Debug, Clone, Copy)]
pub struct OverlayView {
    /// Image width in pixels.
    pub width: u32,
    /// Image height in pixels.
    pub height: u32,
    /// Camera center in WGS84 degrees (`x = lon`, `y = lat`).
    pub center: Coord,
    /// Map zoom level.
    pub zoom: f64,
}
