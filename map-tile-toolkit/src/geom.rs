//! Exact integer geometry predicates shared by clipping, ring orientation, and point-in-ring tests.
//!
//! Every predicate is exact across the full `i32` coordinate range and never panics (no floats, no
//! division). A 2-D cross product of full-`i32` points can reach `2^65`, so exactness needs 128-bit
//! arithmetic *somewhere*; following [`i_overlay`]/`i_float` - which widen `i32 -> i64` and
//! cross-multiply the edge *difference* vectors within a bounded coordinate domain - [`orient`]
//! evaluates the cross in `i64` on the difference vectors and only widens to `i128` when a difference
//! does not fit `i32` (the rare near-full-`i32`-span case). Ordinary tile-local geometry therefore
//! never pays the 128-bit width, while the crate keeps its full-`i32` contract.
//!
//! [`i_overlay`]: https://crates.io/crates/i_overlay

use core::cmp::Ordering;

use geo_types::Coord;

use crate::vertex::Vertex;

/// Orientation of the turn `a -> b -> c`: the sign of the cross product `(b - a) × (c - a)`.
///
/// [`Ordering::Greater`] is a left turn (counter-clockwise), [`Ordering::Less`] a right turn
/// (clockwise), and [`Ordering::Equal`] means the three points are collinear.
#[inline]
pub(crate) fn orient(a: Coord<i32>, b: Coord<i32>, c: Coord<i32>) -> Ordering {
    // Edge vectors as `i64` - an `i32 - i32` difference always fits `i64`.
    let ex1 = i64::from(b.x) - i64::from(a.x);
    let ey1 = i64::from(b.y) - i64::from(a.y);
    let ex2 = i64::from(c.x) - i64::from(a.x);
    let ey2 = i64::from(c.y) - i64::from(a.y);
    // Fast path: when every component fits `i32`, each product is `≤ (2^31 - 1)^2 < 2^62`, so the two
    // `i64` products compare exactly with no risk of overflow. Otherwise widen the two products to
    // `i128`. Comparing the products (rather than subtracting) avoids a possible `i64` overflow in the
    // difference and needs no wider type on the fast path.
    if fits_i32(ex1) && fits_i32(ey1) && fits_i32(ex2) && fits_i32(ey2) {
        (ex1 * ey2).cmp(&(ey1 * ex2))
    } else {
        (i128::from(ex1) * i128::from(ey2)).cmp(&(i128::from(ey1) * i128::from(ex2)))
    }
}

/// Whether an `i64` value fits back into an `i32`.
#[inline]
fn fits_i32(v: i64) -> bool {
    i32::try_from(v).is_ok()
}

/// The smallest integer `≥ x`, where `x` is the point at which segment `a`-`b` crosses the horizontal
/// line at height `y` (`a.y ≠ b.y`, `y` between them). Exact: `x = a.x + dx·t/dy` is evaluated as a
/// rounded-up integer quotient in `i64` when the factors fit `i32`, widening to `i128` otherwise -
/// the same fast path / fallback split as [`orient`]. The result lies between `a.x` and `b.x`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the offset is dx·t/dy with |t| ≤ |dy|, so it fits i64"
)]
pub(crate) fn crossing_x_ceil(a: Coord<i32>, b: Coord<i32>, y: i32) -> i64 {
    let dx = i64::from(b.x) - i64::from(a.x);
    let mut dy = i64::from(b.y) - i64::from(a.y);
    let mut t = i64::from(y) - i64::from(a.y);
    // A positive divisor lets `div_euclid` floor; ceil(n/d) = -floor(-n/d).
    if dy < 0 {
        dy = -dy;
        t = -t;
    }
    let offset = if fits_i32(dx) && fits_i32(t) {
        -(-(dx * t)).div_euclid(dy)
    } else {
        (-(-(i128::from(dx) * i128::from(t))).div_euclid(i128::from(dy))) as i64
    };
    i64::from(a.x) + offset
}

/// Twice the signed area of `ring` (the shoelace sum `Σ xᵢ·yᵢ₊₁ - xᵢ₊₁·yᵢ`), exact for any `i32`
/// ring. Works whether or not the ring repeats its first vertex, and is `0` for a degenerate ring.
///
/// The sign is the ring's winding: **positive** turns counter-clockwise with `y` up - which is
/// *clockwise on screen* with `y` down, i.e. an MVT exterior ring in tile coordinates - and
/// **negative** the other way. The slicer preserves each input ring's winding (synthetic corners
/// follow it), so a caller that needs a fixed convention (MVT: exterior positive, holes negative)
/// normalizes once per feature before slicing, reversing any ring whose sign disagrees with its role.
///
/// Accumulated in `i128`: each term is a cross product of two `i32`-difference vectors (`< 2^65`), so
/// the sum cannot overflow below `2^62` vertices.
#[must_use]
pub fn signed_area_2x<V: Vertex>(ring: &[V]) -> i128 {
    let Some(v0) = ring.first().map(Vertex::position) else {
        return 0;
    };
    // Measured about the first vertex, so the closing edge `v_{n-1} -> v0` and the opening edge
    // `v0 -> v1` both contribute zero: iterating `windows(2)` gives the full doubled area whether or not
    // the ring repeats its first vertex, and smaller magnitudes keep the terms well inside `i128`.
    let mut area2: i128 = 0;
    for w in ring.windows(2) {
        let (p, q) = (w[0].position(), w[1].position());
        let px = i128::from(p.x) - i128::from(v0.x);
        let py = i128::from(p.y) - i128::from(v0.y);
        let qx = i128::from(q.x) - i128::from(v0.x);
        let qy = i128::from(q.y) - i128::from(v0.y);
        area2 += px * qy - py * qx;
    }
    area2
}

/// The winding of a closed `ring` from the sign of [`signed_area_2x`]: [`Ordering::Greater`] for
/// counter-clockwise (y up), [`Ordering::Less`] for clockwise, [`Ordering::Equal`] for a degenerate
/// ring (zero area).
pub(crate) fn ring_orientation<V: Vertex>(ring: &[V]) -> Ordering {
    signed_area_2x(ring).cmp(&0)
}

/// Whether point `p` lies inside `ring` (a closed polygon ring), counting the boundary as **inside**.
///
/// Even-odd (crossing-number) ray cast along `+x`, evaluated with [`orient`] so there is no division
/// and it is exact for all `i32` inputs. A half-open rule on each edge's `y` span (`[y_lo, y_hi)`)
/// counts every crossing once, so a ray grazing a shared vertex is handled consistently. Points on an
/// edge or vertex return `true`. The ring is treated cyclically; a repeated closing vertex is fine.
#[cfg(test)]
pub(crate) fn point_in_ring(p: Coord<i32>, ring: &[Coord<i32>]) -> bool {
    let n = ring.len();
    if n < 3 {
        return false;
    }
    // Drop a repeated closing vertex so the cyclic walk visits each edge once.
    let m = if ring[n - 1] == ring[0] { n - 1 } else { n };
    if m < 3 {
        return false;
    }
    point_in_distinct_ring(p, m, |i| ring[i])
}

/// [`point_in_ring`] over the `m ≥ 3` vertices `at(0..m)` of a ring with no repeated closing vertex.
pub(crate) fn point_in_distinct_ring(p: Coord<i32>, m: usize, at: impl Fn(usize) -> Coord<i32>) -> bool {
    let mut inside = false;
    let mut prev = m - 1;
    for i in 0..m {
        let tail = at(prev);
        let head = at(i);
        // On-boundary check: collinear and within the edge's bounding box -> treat as inside.
        if orient(tail, head, p) == Ordering::Equal
            && p.x >= tail.x.min(head.x)
            && p.x <= tail.x.max(head.x)
            && p.y >= tail.y.min(head.y)
            && p.y <= tail.y.max(head.y)
        {
            return true;
        }
        // Does the edge straddle the horizontal ray at `p.y`? Half-open: exactly one endpoint is
        // strictly above `p.y`.
        if (tail.y > p.y) != (head.y > p.y) {
            // The crossing is to the right of `p` iff `p` is on the correct side of the directed edge.
            // For an upward edge a left turn (`Greater`) puts `p` left of it -> the ray crosses to
            // `p`'s right; a downward edge flips the sense.
            let left = orient(tail, head, p) == Ordering::Greater;
            if left == (head.y > tail.y) {
                inside = !inside;
            }
        }
        prev = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use geo_types::Coord;

    use super::*;

    fn c(x: i32, y: i32) -> Coord<i32> {
        Coord { x, y }
    }

    #[test]
    fn degenerate_ring_contains_nothing() {
        // Fewer than three vertices, or only two distinct ones once the closing copy is dropped.
        assert!(!point_in_ring(c(0, 0), &[c(0, 0), c(5, 5)]));
        assert!(!point_in_ring(c(0, 0), &[c(0, 0), c(5, 5), c(0, 0)]));
    }

    #[test]
    fn orient_basic_and_extremes() {
        assert_eq!(orient(c(0, 0), c(1, 0), c(0, 1)), Ordering::Greater); // CCW / left
        assert_eq!(orient(c(0, 0), c(1, 0), c(0, -1)), Ordering::Less); // CW / right
        assert_eq!(orient(c(0, 0), c(2, 2), c(1, 1)), Ordering::Equal); // collinear
        // Full-`i32`-span vectors take the `i128` fallback and must stay exact.
        assert_eq!(
            orient(c(i32::MIN, i32::MIN), c(i32::MAX, i32::MIN), c(i32::MIN, i32::MAX)),
            Ordering::Greater
        );
        assert_eq!(
            orient(
                c(i32::MIN, i32::MIN),
                c(i32::MAX, i32::MAX),
                c(i32::MIN + 1, i32::MIN + 1)
            ),
            Ordering::Equal
        );
    }

    #[test]
    fn crossing_x_ceil_is_exact() {
        // x = 0 + 10·(1/3) = 3.33… -> 4; reversed direction gives the same point.
        assert_eq!(crossing_x_ceil(c(0, 0), c(10, 3), 1), 4);
        assert_eq!(crossing_x_ceil(c(10, 3), c(0, 0), 1), 4);
        // Exact integer crossings stay put, negative offsets round toward +∞.
        assert_eq!(crossing_x_ceil(c(0, 0), c(10, 2), 1), 5);
        assert_eq!(crossing_x_ceil(c(0, 0), c(-10, 3), 1), -3);
        // Full-`i32` spans take the `i128` path.
        let x = crossing_x_ceil(c(i32::MIN, i32::MIN), c(i32::MAX, i32::MAX), 0);
        assert_eq!(x, 0);
        // The anti-diagonal `x + y = -1` crosses `y = 1` exactly at `x = -2`; one unit to the right
        // of an exact crossing must not round further.
        let x = crossing_x_ceil(c(i32::MIN, i32::MAX), c(i32::MAX, i32::MIN), 1);
        assert_eq!(x, -2);
        let x = crossing_x_ceil(c(i32::MIN, i32::MAX), c(i32::MAX - 1, i32::MIN), 1);
        assert_eq!(x, -2); // the slightly steeper line crosses just left of -2
    }

    #[test]
    fn ring_orientation_ccw_cw() {
        let ccw = [c(0, 0), c(4, 0), c(4, 4), c(0, 4), c(0, 0)];
        let cw = [c(0, 0), c(0, 4), c(4, 4), c(4, 0), c(0, 0)];
        assert_eq!(ring_orientation(&ccw), Ordering::Greater);
        assert_eq!(ring_orientation(&cw), Ordering::Less);
        assert_eq!(ring_orientation(&ccw[..2]), Ordering::Equal); // degenerate
        // Open and closed rings agree, and the magnitude is twice the area.
        assert_eq!(signed_area_2x(&ccw), 32);
        assert_eq!(signed_area_2x(&ccw[..4]), 32);
        assert_eq!(signed_area_2x(&cw), -32);
        assert_eq!(signed_area_2x::<Coord<i32>>(&[]), 0);
        // Full-`i32` extremes stay exact.
        let big = [c(i32::MIN, i32::MIN), c(i32::MAX, i32::MIN), c(i32::MAX, i32::MAX)];
        let side = i128::from(i32::MAX) - i128::from(i32::MIN);
        assert_eq!(signed_area_2x(&big), side * side);
    }

    #[test]
    fn point_in_ring_square() {
        let sq = [c(0, 0), c(10, 0), c(10, 10), c(0, 10), c(0, 0)];
        assert!(point_in_ring(c(5, 5), &sq)); // interior
        assert!(!point_in_ring(c(15, 5), &sq)); // exterior
        assert!(point_in_ring(c(0, 5), &sq)); // on the left edge -> boundary counts as inside
        assert!(point_in_ring(c(10, 10), &sq)); // corner vertex
        assert!(!point_in_ring(c(-1, 5), &sq));
    }

    #[test]
    fn point_in_ring_concave() {
        // A C-shape (concave): the notch on the right is outside.
        let c_shape = [
            c(0, 0),
            c(10, 0),
            c(10, 3),
            c(3, 3),
            c(3, 7),
            c(10, 7),
            c(10, 10),
            c(0, 10),
            c(0, 0),
        ];
        assert!(point_in_ring(c(1, 5), &c_shape)); // in the spine
        assert!(!point_in_ring(c(6, 5), &c_shape)); // in the notch -> outside
        assert!(point_in_ring(c(6, 1), &c_shape)); // in the bottom arm
    }
}
