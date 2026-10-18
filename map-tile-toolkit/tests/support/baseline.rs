//! Traditional tile clippers to compare the toolkit against, on the same inputs (used by the
//! equivalence check in `tests/baseline.rs`; never part of the library):
//!
//! * **geo** — [`geo`]'s `BooleanOps` (`intersection` for polygons, `clip` for lines) against each
//!   tile's buffered box. A general sweep-line overlay, so it is an *upper bound*, not a target; it
//!   needs only [`tile_rect`].
//! * **stripe** — a geojson-vt-style axis-stripe clipper ([`clip`], [`clip_tile`], [`clip_all`]):
//!   Sutherland–Hodgman for rings, split parts for lines, every ring clipped on its own. This is the
//!   realistic "traditional" target.
//!
//! Both clip in `f64` to the box the toolkit clips to: a tile's integer cell grown by the buffer.
//! Unlike the toolkit they compute intersection points rather than keeping the original vertices.

#![allow(
    dead_code,
    reason = "shared by the bench and test crates; not every helper is used in each"
)]

use geo_types::{Coord, Rect};
use map_tile_toolkit::TileId;

/// A point in `f64` tile space.
pub type Pt = [f64; 2];

/// Tile `t`'s buffered span on one axis: `[t·extent − buffer, (t+1)·extent − 1 + buffer]`.
#[must_use]
pub fn tile_span(t: i32, extent: u32, buffer: u16) -> (f64, f64) {
    let lo = f64::from(t) * f64::from(extent) - f64::from(buffer);
    (lo, lo + f64::from(extent) - 1.0 + 2.0 * f64::from(buffer))
}

/// A tile's buffered box as a `geo` rectangle.
#[must_use]
pub fn tile_rect(tile: TileId, extent: u32, buffer: u16) -> Rect<f64> {
    let ((x0, x1), (y0, y1)) = (
        tile_span(tile.x, extent, buffer),
        tile_span(tile.y, extent, buffer),
    );
    Rect::new(Coord { x: x0, y: y0 }, Coord { x: x1, y: y1 })
}

// ---- The stripe clipper ----

/// Clip `pts` to `lo..=hi` on `axis` (0 = x, 1 = y), appending to `out`: a `closed` ring yields at
/// most one ring (Sutherland–Hodgman, unclosed), an open line one part per pass through the stripe.
pub fn clip(pts: &[Pt], axis: usize, lo: f64, hi: f64, closed: bool, out: &mut Vec<Vec<Pt>>) {
    let mut part = Vec::new();
    for (i, &a) in pts.iter().enumerate() {
        let ak = a[axis];
        if (lo..=hi).contains(&ak) {
            part.push(a);
        }
        let Some(&b) = pts.get(i + 1).or(if closed { pts.first() } else { None }) else {
            break;
        };
        let (bk, up) = (b[axis], ak < b[axis]);
        // Crossing a bound goes between inside (`lo..=hi`, edges included) and outside.
        let mut bounds = [
            (lo, (ak < lo) != (bk < lo), false),
            (hi, (ak > hi) != (bk > hi), true),
        ];
        if !up {
            bounds.reverse();
        }
        for (k, crosses, is_hi) in bounds {
            if crosses {
                let mut p = a;
                p[axis] = k;
                p[1 - axis] += (b[1 - axis] - a[1 - axis]) * (k - ak) / (bk - ak);
                part.push(p);
                // Moving up past `hi` or down past `lo` leaves the stripe, ending an open line's part.
                if !closed && is_hi == up {
                    out.push(std::mem::take(&mut part));
                }
            }
        }
    }
    if part.len() >= if closed { 3 } else { 2 } {
        out.push(part);
    }
}

/// Clip every part to one tile's buffered box: the x stripe, then each piece to the y stripe.
pub fn clip_tile(
    parts: &[Vec<Pt>],
    tile: TileId,
    extent: u32,
    buffer: u16,
    closed: bool,
    out: &mut Vec<Vec<Pt>>,
) {
    let ((x0, x1), (y0, y1)) = (
        tile_span(tile.x, extent, buffer),
        tile_span(tile.y, extent, buffer),
    );
    let mut column = Vec::new();
    for part in parts {
        clip(part, 0, x0, x1, closed, &mut column);
    }
    for part in &column {
        clip(part, 1, y0, y1, closed, out);
    }
}

/// Clip every part into each of `tiles` (sorted by column, as [`TileId`] orders), geojson-vt style:
/// cut the parts into one stripe per column, then each stripe into its tiles' rows, calling `emit`
/// for each non-empty tile.
pub fn clip_all(
    parts: &[Vec<Pt>],
    tiles: &[TileId],
    extent: u32,
    buffer: u16,
    closed: bool,
    mut emit: impl FnMut(TileId, &[Vec<Pt>]),
) {
    let (mut column, mut cell) = (Vec::new(), Vec::new());
    for col in tiles.chunk_by(|a, b| a.x == b.x) {
        let (x0, x1) = tile_span(col[0].x, extent, buffer);
        column.clear();
        for part in parts {
            clip(part, 0, x0, x1, closed, &mut column);
        }
        for &tile in col {
            let (y0, y1) = tile_span(tile.y, extent, buffer);
            cell.clear();
            for part in &column {
                clip(part, 1, y0, y1, closed, &mut cell);
            }
            if !cell.is_empty() {
                emit(tile, &cell);
            }
        }
    }
}
