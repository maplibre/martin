//! Per-shape construction of [`Shape`] / [`Marker`] from `GeoJSON`
//! geometries plus their simplestyle properties.

use geo_types::Coord;
use geojson::{JsonObject, Position};

use crate::overlay::parse::OverlayParseError;
use crate::overlay::parse::simplestyle::{
    DEFAULT_COLOR_STR, DEFAULT_FILL_OPACITY, f64_prop, parse_color_with_opacity, resolve_stroke,
    str_prop,
};
use crate::overlay::{Fill, Marker, MarkerStyle, Shape, Stroke};

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

pub(super) fn make_line(
    positions: &[Position],
    props: Option<&JsonObject>,
) -> Result<Option<Shape>, OverlayParseError> {
    let Some(points) = ring_coords(positions)? else {
        return Ok(None);
    };
    let (color, width) = resolve_stroke(props, DEFAULT_COLOR_STR)?;
    Ok(Some(Shape::Line {
        points,
        stroke: Stroke::new(color, width),
    }))
}

pub(super) fn make_polygon(
    rings: &[Vec<Position>],
    props: Option<&JsonObject>,
) -> Result<Option<Shape>, OverlayParseError> {
    let mut iter = rings.iter();
    let Some(outer_ring) = iter.next() else {
        return Ok(None);
    };
    let Some(outer) = ring_coords(outer_ring)? else {
        return Ok(None);
    };
    let mut holes: Vec<Vec<Coord>> = Vec::new();
    for ring in iter {
        if let Some(coords) = ring_coords(ring)? {
            holes.push(coords);
        }
    }

    let fill_color_str = str_prop(props, "fill").unwrap_or(DEFAULT_COLOR_STR);
    let fill_opacity = f64_prop(props, "fill-opacity")?.unwrap_or(DEFAULT_FILL_OPACITY);
    let fill_color = parse_color_with_opacity("fill", fill_color_str, fill_opacity)?;

    // Polygons default `stroke` to the fill color rather than `DEFAULT_COLOR_STR`,
    // so a fill-only polygon doesn't gain a contrasting outline.
    let (stroke_color, stroke_width) = resolve_stroke(props, fill_color_str)?;

    Ok(Some(Shape::Polygon {
        outer,
        holes,
        stroke: Some(Stroke::new(stroke_color, stroke_width)),
        fill: Some(Fill::new(fill_color)),
    }))
}

pub(super) fn make_marker(
    coord: Coord,
    props: Option<&JsonObject>,
) -> Result<Marker, OverlayParseError> {
    let style = match str_prop(props, "marker-color") {
        Some(c) => MarkerStyle {
            color: parse_color_with_opacity("marker-color", c, 1.0)?,
            ..MarkerStyle::DEFAULT
        },
        None => MarkerStyle::DEFAULT,
    };
    Ok(Marker { coord, style })
}
