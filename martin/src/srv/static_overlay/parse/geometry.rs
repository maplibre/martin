//! Per-shape construction of [`PathOverlay`] / [`MarkerOverlay`] from
//! `GeoJSON` geometries plus their simplestyle properties.

use geo_types::Coord;
use geojson::{JsonObject, Position};

use crate::srv::static_overlay::parse::OverlayParseError;
use crate::srv::static_overlay::parse::simplestyle::{
    DEFAULT_COLOR, DEFAULT_FILL_OPACITY, f64_prop, paint_with_opacity, str_prop, stroke_paint,
};
use crate::srv::static_overlay::{MarkerOverlay, PathOverlay};

/// `GeoJSON` Positions are `[lng, lat, …]` per RFC 7946 § 3.1.1.
/// The geojson crate skips length validation on non-Point Positions, so the bounds check is load-bearing.
pub(super) fn to_coord(pos: &Position) -> Result<Coord, OverlayParseError> {
    if pos.len() < 2 {
        return Err(OverlayParseError::PositionTooShort {
            position: pos.as_slice().to_vec(),
        });
    }
    Ok(Coord {
        x: pos[0],
        y: pos[1],
    })
}

/// Convert a ring of `GeoJSON` positions to `Coord`s, dropping rings with < 2 points.
fn ring_coords(positions: &[Position]) -> Result<Option<Vec<Coord>>, OverlayParseError> {
    let coords = positions
        .iter()
        .map(to_coord)
        .collect::<Result<Vec<Coord>, _>>()?;
    Ok((coords.len() >= 2).then_some(coords))
}

pub(super) fn make_path(
    positions: &[Position],
    props: Option<&JsonObject>,
) -> Result<Option<PathOverlay>, OverlayParseError> {
    let Some(points) = ring_coords(positions)? else {
        return Ok(None);
    };
    let (stroke, width) = stroke_paint(props, DEFAULT_COLOR)?;
    Ok(Some(PathOverlay {
        points,
        holes: Vec::new(),
        stroke,
        fill: None,
        width,
    }))
}

pub(super) fn make_polygon(
    rings: &[Vec<Position>],
    props: Option<&JsonObject>,
) -> Result<Option<PathOverlay>, OverlayParseError> {
    let mut iter = rings.iter();
    let Some(outer) = iter.next() else {
        return Ok(None);
    };
    let Some(points) = ring_coords(outer)? else {
        return Ok(None);
    };
    let mut holes: Vec<Vec<Coord>> = Vec::new();
    for ring in iter {
        if let Some(coords) = ring_coords(ring)? {
            holes.push(coords);
        }
    }
    let fill_color = str_prop(props, "fill").unwrap_or(DEFAULT_COLOR);
    let fill_opacity = f64_prop(props, "fill-opacity").unwrap_or(DEFAULT_FILL_OPACITY);
    let fill = Some(paint_with_opacity("fill", fill_color, fill_opacity)?);
    // Polygon strokes default to the fill color (not gray) so a fill-only
    // polygon renders as a clean shape without a contrasting border.
    let (stroke, width) = stroke_paint(props, fill_color)?;
    Ok(Some(PathOverlay {
        points,
        holes,
        stroke,
        fill,
        width,
    }))
}

pub(super) fn make_marker(
    coord: Coord,
    props: Option<&JsonObject>,
) -> Result<MarkerOverlay, OverlayParseError> {
    let marker_color = match str_prop(props, "marker-color") {
        Some(c) => Some(paint_with_opacity("marker-color", c, 1.0)?),
        None => None,
    };
    Ok(MarkerOverlay {
        coord,
        marker_color,
    })
}
