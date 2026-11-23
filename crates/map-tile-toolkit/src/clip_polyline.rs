//! Low-level polyline clipping against a single tile box (the engine behind line slicing).
//!
//! Clipping keeps the **original** vertices - never cutting new ones at the tile edge. Every
//! segment that touches the box contributes both of its endpoints, so a line shows up in every tile
//! it passes through, even ones it merely crosses with no vertex inside. A stretch that leaves the
//! box and re-enters splits into separate pieces only where a vertex is *dropped*: a single-segment
//! excursion out and back keeps both its (outside) endpoints, so it stays one connected piece.

use core::cmp::Ordering;

use geo_types::Coord;

use crate::TileError;
use crate::geom::orient;
use crate::vertex::Vertex;

/// Is coordinate `c` inside the closed rectangle `[min, max]`?
#[inline]
fn inside(c: Coord<i32>, min: Coord<i32>, max: Coord<i32>) -> bool {
    c.x >= min.x && c.x <= max.x && c.y >= min.y && c.y <= max.y
}

/// Does segment `a`-`b` touch the closed integer rectangle `[min, max]`?
///
/// Integer-only (no division, no floats): reject when the segment's bounding box is disjoint from
/// the box, accept when an endpoint is inside, otherwise test whether the box straddles the
/// segment's supporting line via i128 cross products (so full `i32` coordinates cannot overflow).
#[inline]
pub(crate) fn segment_intersects(a: Coord<i32>, b: Coord<i32>, min: Coord<i32>, max: Coord<i32>) -> bool {
    // Quick reject: the segment's bounding box is disjoint from the tile box.
    if a.x.min(b.x) > max.x || a.x.max(b.x) < min.x || a.y.min(b.y) > max.y || a.y.max(b.y) < min.y {
        return false;
    }
    // Quick accept: an endpoint lies inside the (closed) box.
    inside(a, min, max) || inside(b, min, max) || line_meets_box(a, b, min, max)
}

/// The last step of [`segment_intersects`], for a segment whose endpoints are both outside the box but
/// whose bounding box overlaps it: the segment then meets the box iff the box's four corners are not
/// all strictly on one side of its supporting line (each side is the orientation of `a -> b -> corner`).
pub(crate) fn line_meets_box(a: Coord<i32>, b: Coord<i32>, min: Coord<i32>, max: Coord<i32>) -> bool {
    let s = [
        orient(a, b, min),
        orient(a, b, Coord { x: max.x, y: min.y }),
        orient(a, b, Coord { x: min.x, y: max.y }),
        orient(a, b, max),
    ];
    !(s.iter().all(|&v| v == Ordering::Greater) || s.iter().all(|&v| v == Ordering::Less))
}

/// Re-express vertex `v` in the tile-local frame whose `[0, 0]` corner is `origin`: its position
/// becomes `position - origin`, its payload unchanged. [`TileError::Overflow`] if the offset leaves the
/// `i32` range - possible only when a far crossing-segment endpoint lies more than a full `i32` span
/// from the tile.
pub(crate) fn to_local<V: Vertex>(v: V, origin: Coord<i32>) -> Result<V, TileError> {
    let p = v.position();
    Ok(v.with_position(Coord {
        x: p.x.checked_sub(origin.x).ok_or(TileError::Overflow)?,
        y: p.y.checked_sub(origin.y).ok_or(TileError::Overflow)?,
    }))
}
