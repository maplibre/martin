//! What `ST_AsMVTGeom` keeps when it clips: a line or polygon that meets a tile's buffered box only on its
//! edge is dropped there, as its intersection with the box is of lower dimension than the geometry.

use crate::sort::record::Vertex;

/// A tile's closed buffered box `[lo, hi]` on both axes of its frame.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TileBox {
    lo: i32,
    hi: i32,
}

impl TileBox {
    /// The box `[-buffer, extent + buffer]` of a tile with this `extent` and `buffer`.
    pub(crate) fn new(extent: i32, buffer: i32) -> Self {
        Self {
            lo: -buffer,
            hi: extent + buffer,
        }
    }

    /// Whether some segment of the tile-local `run` lies in the box along a length, not only at points.
    pub(crate) fn holds_length(self, run: &[Vertex]) -> bool {
        run.windows(2).any(|w| self.meets(w[0], w[1], false))
    }

    /// Whether the polygon of the open, tile-local `rings` covers some area of the box: an edge passes through
    /// its interior, or no edge does and the interior lies inside the polygon.
    pub(crate) fn holds_area<'a>(self, rings: impl Iterator<Item = &'a [Vertex]> + Clone) -> bool {
        rings
            .clone()
            .any(|ring| edges(ring).any(|(a, b)| self.meets(a, b, true)))
            || self.center_inside(rings)
    }

    /// Whether the segment `a`-`b` meets the box along a length (`open == false`) or meets its interior.
    fn meets(self, a: Vertex, b: Vertex, open: bool) -> bool {
        // The segment is `a + t·(b - a)` for `t` in `[0, 1]`; each axis narrows `t` to where it is in the box,
        // the bounds kept as fractions `n / d` with `d > 0`.
        let (mut from, mut to) = ((0_i128, 1_i128), (1_i128, 1_i128));
        let (min, max) = (i128::from(self.lo), i128::from(self.hi));
        for axis in 0..2 {
            let (p, d) = (i128::from(a[axis]), i128::from(b[axis]) - i128::from(a[axis]));
            if d == 0 {
                let inside = if open { min < p && p < max } else { min <= p && p <= max };
                if !inside {
                    return false;
                }
                continue;
            }
            let (enter, leave, den) = if d > 0 {
                (min - p, max - p, d)
            } else {
                (p - max, p - min, -d)
            };
            if enter * from.1 > from.0 * den {
                from = (enter, den);
            }
            if leave * to.1 < to.0 * den {
                to = (leave, den);
            }
        }
        from.0 * to.1 < to.0 * from.1
    }

    /// Whether the box's center, which no edge passes, lies inside the polygon (even-odd).
    fn center_inside<'a>(self, rings: impl Iterator<Item = &'a [Vertex]>) -> bool {
        let c = i128::from(self.lo + self.hi).div_euclid(2);
        let mut inside = false;
        for (a, b) in rings.flat_map(edges) {
            let (ax, ay, bx, by) = (i128::from(a[0]), i128::from(a[1]), i128::from(b[0]), i128::from(b[1]));
            if (ay > c) != (by > c) {
                // The edge crosses `y = c` right of the center when `x - c` there has the sign of `by - ay`.
                let right = (ax - c) * (by - ay) + (c - ay) * (bx - ax);
                inside ^= (right > 0) == (by > ay);
            }
        }
        inside
    }
}

/// The edges of an open ring, the last closing it.
fn edges(ring: &[Vertex]) -> impl Iterator<Item = (Vertex, Vertex)> + '_ {
    ring.iter().zip(ring.iter().cycle().skip(1)).map(|(&a, &b)| (a, b))
}
