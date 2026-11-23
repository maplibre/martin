//! Clipping one polygon **ring** to a single tile's buffered box, keeping the **original** vertices
//! and closing the result with synthetic clip-boundary corners so the ring stays a closed, correctly
//! wound loop. The engine behind polygon slicing.
//!
//! A vertex is kept by the same rule as the polyline clip (an incident edge touches the box); the
//! dropped outside excursions between kept arcs are replaced by detours that hug the box exterior
//! (`B⁺`, one unit outside `B`) via its corners. Each detour is built to be **homotopic** to the
//! excursion it replaces - it reproduces the excursion's winding around the box center - so the fill
//! inside the box is exact even when a ring wraps the tile. All synthetic geometry lies strictly
//! outside `B`, which lets [`Mosaic`](crate::Mosaic) recognise and drop it geometrically.
//!
//! [`close_ring`] is the shared core: given which edges touch the box and a way to measure an
//! excursion's winding, it closes the kept arcs. [`clip_ring`] (the single-tile clip) finds the
//! touching edges and windings by walking the whole ring; the all-tiles slicer feeds it the edges its
//! routing pass found and windings from its per-row crossing index, so both produce identical rings.

use core::cmp::Ordering;
use core::iter;

use geo_types::Coord;

use crate::TileError;
use crate::clip_polyline::{line_meets_box, segment_intersects};
use crate::geom::{orient, point_in_distinct_ring, ring_orientation};
use crate::vertex::{PolyVertex, Vertex};

// Cohen-Sutherland outcode bits for a point vs the inclusive box `[min, max]`.
const LEFT: u8 = 1;
const RIGHT: u8 = 2;
const BOTTOM: u8 = 4;
const TOP: u8 = 8;
/// All four outside bits.
const BOX: u8 = LEFT | RIGHT | BOTTOM | TOP;
/// Not an outcode bit: set beside one for a point above the winding ray's line (`y > cy`).
const ABOVE: u8 = 16;

/// Outcode of `p` relative to the closed box `[min, max]` (0 iff `p` is inside).
fn outcode(p: Coord<i32>, min: Coord<i32>, max: Coord<i32>) -> u8 {
    let mut oc = 0;
    if p.x < min.x {
        oc |= LEFT;
    } else if p.x > max.x {
        oc |= RIGHT;
    }
    if p.y < min.y {
        oc |= BOTTOM;
    } else if p.y > max.y {
        oc |= TOP;
    }
    oc
}

/// Boundary slot `0..8` of an **outside** point, counter-clockwise with y up:
/// `0=SW, 1=S, 2=SE, 3=E, 4=NE, 5=N, 6=NW, 7=W`. Corners are the even slots. A zero outcode (inside)
/// cannot occur for a detour endpoint; it maps to `0` defensively so nothing can panic.
fn slot(oc: u8) -> u8 {
    match oc {
        _ if oc == BOTTOM | LEFT => 0,
        BOTTOM => 1,
        _ if oc == BOTTOM | RIGHT => 2,
        RIGHT => 3,
        _ if oc == RIGHT | TOP => 4,
        TOP => 5,
        _ if oc == TOP | LEFT => 6,
        LEFT => 7,
        _ => 0,
    }
}

/// How [`close_ring`] bridges a ring's dropped gaps. The default is the plain clip: each gap becomes a
/// chord, a corner detour or the verbatim gap, whichever is cheapest. A repair attempt (see
/// `valid_piece`) asks for more: `deepen` original vertices kept past each tip of an excursion, and
/// detour corners moved `lane` units further out, so concurrent detours don't share a line.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RingPlan<'a> {
    /// Original vertices kept past each tip into every gap; `usize::MAX` keeps every gap whole.
    pub(crate) deepen: usize,
    /// The first bridge's lane, and how much each further bridge of the ring moves out.
    pub(crate) lane_base: i32,
    pub(crate) lane_step: i32,
    /// Close every excursion on a frame around this convex hull (of everything the piece keeps
    /// outside the box, and `B⁺`) instead of hugging `B⁺`: each tip leaves the hull through a short
    /// stub, which nothing kept can cross, and runs along the hull grown by the lane. Needs every
    /// tip to be a vertex of the hull, else the attempt fails.
    pub(crate) frame: Option<&'a [Coord<i32>]>,
    /// Per bridge of the piece (its excursions, in ring then arc order), how many vertices to keep
    /// past the tip before it and after the one behind it, instead of `deepen` for all.
    pub(crate) depths: Option<&'a [(u32, u32)]>,
    /// The index among the piece's bridges of this ring's first.
    pub(crate) bridge_base: usize,
}

impl RingPlan<'_> {
    /// The plain clip: nothing kept past the tips, every detour hugging `B⁺`.
    pub(crate) const PLAIN: RingPlan<'static> = RingPlan {
        deepen: 0,
        lane_base: 0,
        lane_step: 0,
        frame: None,
        depths: None,
        bridge_base: 0,
    };

    /// How many vertices of a gap of `gap` to keep after the tip before it, and before the tip after
    /// it, for the piece's bridge number `bridge`.
    fn depth(&self, bridge: usize, gap: usize) -> (usize, usize) {
        let (head, tail) = match self.depths {
            Some(d) => d.get(bridge).map_or((0, 0), |&(h, t)| (h as usize, t as usize)),
            None => (self.deepen, self.deepen),
        };
        let head = head.min(gap);
        (head, tail.min(gap - head))
    }
}

/// `[min, max]` grown by `lane` units per side - the box detour corners are placed around.
pub(crate) fn lane_bounds(min: Coord<i32>, max: Coord<i32>, lane: i32) -> Result<(Coord<i32>, Coord<i32>), TileError> {
    let g = |v: i32, d: i32| v.checked_add(d).ok_or(TileError::Overflow);
    Ok((
        Coord {
            x: g(min.x, -lane)?,
            y: g(min.y, -lane)?,
        },
        Coord {
            x: g(max.x, lane)?,
            y: g(max.y, lane)?,
        },
    ))
}

/// Whether every vertex of `pts` is in the box `[min, max]`. An offset from `min` is inside iff, as
/// `u32` (so below zero wraps far past it), it is at most the box's span: one comparison per axis,
/// and one branch per vertex.
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "the plain clip's first pass; out of line it costs more than it does"
)]
fn all_inside<V: Vertex>(pts: &[V], min: Coord<i32>, max: Coord<i32>) -> bool {
    let span = |lo: i32, hi: i32| hi.wrapping_sub(lo).cast_unsigned();
    let (sx, sy) = (span(min.x, max.x), span(min.y, max.y));
    pts.iter().all(|p| {
        let p = p.position();
        (span(min.x, p.x) <= sx) & (span(min.y, p.y) <= sy)
    })
}

/// The `B⁺` corner at an even `slot`, one unit outside the box. [`TileError::Overflow`] if the tile
/// sits so close to the `i32` edge that a corner can't be placed strictly outside it.
fn corner(slot: u8, min: Coord<i32>, max: Coord<i32>) -> Result<Coord<i32>, TileError> {
    let lo_x = min.x.checked_sub(1).ok_or(TileError::Overflow)?;
    let lo_y = min.y.checked_sub(1).ok_or(TileError::Overflow)?;
    let hi_x = max.x.checked_add(1).ok_or(TileError::Overflow)?;
    let hi_y = max.y.checked_add(1).ok_or(TileError::Overflow)?;
    Ok(match slot {
        0 => Coord { x: lo_x, y: lo_y }, // SW⁺
        2 => Coord { x: hi_x, y: lo_y }, // SE⁺
        4 => Coord { x: hi_x, y: hi_y }, // NE⁺
        _ => Coord { x: lo_x, y: hi_y }, // NW⁺ (slot 6)
    })
}

/// Signed crossing of segment `a -> b` with the ray `{ y = cy, x > max_x }` - the part of the `+x` ray
/// from the box center that lies outside the box: `+1` upward, `-1` downward, `0` if it misses
/// (half-open in `y`, so a vertex shared by two edges is counted once). Summed along a path this is
/// the path's winding around the box center; since every excursion and every detour lives outside the
/// box, only the outside portion of the ray can be crossed.
fn ray_crossing(a: Coord<i32>, b: Coord<i32>, cy: i32, max_x: i32) -> i64 {
    if (a.y > cy) == (b.y > cy) {
        return 0;
    }
    // Crossing `x` compared to `max_x` without division: `x - max_x = num / dy`.
    let dy = i128::from(b.y) - i128::from(a.y);
    let num = (i128::from(a.x) - i128::from(max_x)) * dy
        + (i128::from(b.x) - i128::from(a.x)) * (i128::from(cy) - i128::from(a.y));
    // `x > max_x` iff `num` and `dy` share a sign (their product is positive).
    if num.signum() * dy.signum() > 0 {
        if b.y > a.y { 1 } else { -1 }
    } else {
        0
    }
}

/// [`ray_crossing`] summed over a path's consecutive points.
fn ray_crossings(path: impl IntoIterator<Item = Coord<i32>>, cy: i32, max_x: i32) -> i64 {
    let mut path = path.into_iter();
    let Some(mut prev) = path.next() else {
        return 0;
    };
    let mut w = 0;
    for p in path {
        w += ray_crossing(prev, p, cy, max_x);
        prev = p;
    }
    w
}

/// Corner slots visited walking `advance` slots (CCW if positive, CW if negative) from `start`: every
/// even slot passed except the final one (where the detour's other endpoint sits). `advance == 0`
/// yields none (a direct chord).
fn corner_slots(start: u8, advance: i64) -> impl Iterator<Item = u8> {
    // One slot CCW (`+1`) or CW (`+7 ≡ -1`) around the 8-slot loop, staying in `u8`.
    let step = if advance > 0 { 1 } else { 7 };
    let mut s = start;
    (1..advance.unsigned_abs()).filter_map(move |_| {
        s = (s + step) % 8;
        s.is_multiple_of(2).then_some(s)
    })
}

/// How many corners [`corner_slots`] yields, in O(1): of the `|advance| - 1` intermediate slots,
/// every other one is a corner, starting with the first when `start` is odd.
fn corner_count(start: u8, advance: i64) -> u64 {
    let n = advance.unsigned_abs().saturating_sub(1);
    if start.is_multiple_of(2) { n / 2 } else { n.div_ceil(2) }
}

/// The box's center `y` (overflow-safe midpoint) - the height of the winding ray.
pub(crate) fn center_y(min: Coord<i32>, max: Coord<i32>) -> i32 {
    i32::midpoint(min.y, max.y)
}

/// How to bridge one dropped gap between kept arcs.
enum Bridge {
    /// Keep the gap's original vertices verbatim (no more of them than a detour would need).
    Keep,
    /// Replace the gap with the synthetic `B⁺` corners from [`corner_slots`]`(start, advance)`.
    Detour { start: u8, advance: i64 },
}

/// Decide how to bridge a gap of `gap_len` dropped vertices from exit vertex `u` to entry vertex `v`
/// (both outside the box) whose excursion winds `w_exc` times around the box center ([`ray_crossing`]
/// summed over its edges). The detour reproduces that winding, so it is homotopic to the excursion in
/// the box exterior: the CCW corner path from `u`'s slot to `v`'s is the reference, and whole `∂B⁺`
/// loops (±8 slots each) make up the winding difference.
///
/// Every gap vertex is outside the box (both its edges miss it - that's why it was dropped), so keeping
/// the gap verbatim reproduces the real excursion with its exact winding, entirely outside the box.
/// Synthesizing only ever trades those originals for `B⁺` corners, so it's worth doing only when it
/// *saves* vertices - a longer excursion that would otherwise drag its far geometry into this tile.
/// Either way `Mosaic` recovers the real geometry from the tiles that own it.
fn plan_bridge(
    u: Coord<i32>,
    v: Coord<i32>,
    w_exc: i64,
    gap_len: usize,
    min: Coord<i32>,
    max: Coord<i32>,
    cy: i32,
) -> Result<Bridge, TileError> {
    plan_bridge_in(u, v, w_exc, gap_len, (min, max), (min, max), cy)
}

/// [`plan_bridge`] for a repair, whose detour corners go around `(cmin, cmax)`, its lane, rather than
/// the box `(min, max)`.
fn plan_bridge_lane(
    u: Coord<i32>,
    v: Coord<i32>,
    w_exc: i64,
    gap_len: usize,
    bounds: (Coord<i32>, Coord<i32>),
    lane: (Coord<i32>, Coord<i32>),
    cy: i32,
) -> Result<Bridge, TileError> {
    plan_bridge_in(u, v, w_exc, gap_len, bounds, lane, cy)
}

/// [`plan_bridge`] and [`plan_bridge_lane`] in one body, so the plain clip's, the hot one, takes no
/// more arguments than it needs.
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "each caller is one instance; the lane's arguments cost the plain clip ~1%"
)]
fn plan_bridge_in(
    u: Coord<i32>,
    v: Coord<i32>,
    w_exc: i64,
    gap_len: usize,
    (min, max): (Coord<i32>, Coord<i32>),
    (cmin, cmax): (Coord<i32>, Coord<i32>),
    cy: i32,
) -> Result<Bridge, TileError> {
    let su = slot(outcode(u, min, max));
    let sv = slot(outcode(v, min, max));
    let base = i64::from((sv + 8 - su) % 8); // 0..8, CCW distance su -> sv
    // Winding of the reference detour (at most three corners).
    let mut w_ref = 0;
    let mut prev = u;
    for s in corner_slots(su, base) {
        let c = corner(s, cmin, cmax)?;
        w_ref += ray_crossing(prev, c, cy, max.x);
        prev = c;
    }
    w_ref += ray_crossing(prev, v, cy, max.x);
    let advance = (w_exc - w_ref)
        .checked_mul(8)
        .and_then(|loops| loops.checked_add(base))
        .ok_or(TileError::Overflow)?;
    let keep = u64::try_from(gap_len).is_ok_and(|g| g <= corner_count(su, advance));
    Ok(if keep {
        Bridge::Keep
    } else {
        Bridge::Detour { start: su, advance }
    })
}

/// Append to `out` the distinct vertices of `ring` in cyclic order - consecutive duplicate positions
/// and a repeated closing vertex dropped, so zero-length edges can't distort the clip (matching the
/// polyline slicer's handling of consecutive duplicates). Returns how many were appended. Callers
/// borrow a ring that is already distinct (see [`distinct_prefix`]) and copy only when it is not.
#[inline]
pub(crate) fn push_distinct<V: Vertex>(ring: &[V], out: &mut Vec<V>) -> usize {
    let start = out.len();
    for &v in ring {
        if out.len() == start || out.last().map(Vertex::position) != Some(v.position()) {
            out.push(v);
        }
    }
    while out.len() - start >= 2 && out.last().map(Vertex::position) == Some(out[start].position()) {
        out.pop();
    }
    out.len() - start
}

/// The closed fill-box ring through the `B⁺` corners `sw` and `ne`, oriented like `orient`.
pub(crate) fn fill_box(orient: Ordering, sw: Coord<i32>, ne: Coord<i32>) -> [Coord<i32>; 5] {
    let se = Coord { x: ne.x, y: sw.y };
    let nw = Coord { x: sw.x, y: ne.y };
    // Counter-clockwise for a CCW ring, clockwise otherwise, so the fill matches the ring's sense.
    if orient == Ordering::Less {
        [sw, nw, ne, se, sw]
    } else {
        [sw, se, ne, nw, sw]
    }
}

/// Extend the kept arcs with touching edge `t` (edges must come in ascending order). Arcs are inclusive
/// vertex spans `(start, end)`, `end` unwrapped (it may pass the ring's length by wrapping to its
/// start). Edge `t` keeps vertices `t` and `t + 1`, so two touching edges at most two apart keep one
/// contiguous arc (the edge between them is a single-segment out-and-back bridge).
#[inline]
pub(crate) fn keep_edge(arcs: &mut Vec<(usize, usize)>, t: usize) {
    match arcs.last_mut() {
        Some((_, end)) if t <= *end + 1 => *end = t + 1,
        _ => arcs.push((t, t + 1)),
    }
}

/// Reusable working memory for [`clip_ring`]: it holds nothing between calls, only its capacity
/// survives one.
pub(crate) struct ClipScratch<V> {
    copy: Vec<V>,
    crossings: Vec<(u32, i64)>,
    arcs: Vec<(usize, usize)>,
    tmp: FrameTmp,
}

impl<V> ClipScratch<V> {
    pub(crate) const fn new() -> Self {
        Self {
            copy: Vec::new(),
            crossings: Vec::new(),
            arcs: Vec::new(),
            tmp: FrameTmp::new(),
        }
    }
}

impl<V: PolyVertex> ClipScratch<V> {
    /// Start a repair attempt: its frame's hull is not grown yet.
    #[inline]
    pub(crate) fn start_attempt(&mut self) {
        self.tmp.grown_by = None;
    }

    /// The plain clip's [`close_ring`](fn@close_ring), for a caller that found the ring's `arcs`
    /// itself (the all-tiles slicer).
    #[inline]
    pub(crate) fn close_ring(
        &mut self,
        ring: &[V],
        arcs: &mut [(usize, usize)],
        (min, max): (Coord<i32>, Coord<i32>),
        winding: impl FnMut(usize, usize) -> i64,
        piece: &mut PieceBuf<V>,
    ) -> Result<(), TileError> {
        // (The plain clip has no frame to miss.)
        let plain = &RingPlan::PLAIN;
        close_ring::<true, V>(ring, arcs, min, max, winding, plain, &mut self.tmp, piece)?;
        Ok(())
    }

    /// [`Self::close_ring`] for a repair, closed by `plan`.
    #[inline]
    pub(crate) fn close_ring_planned(
        &mut self,
        ring: &[V],
        arcs: &mut [(usize, usize)],
        (min, max): (Coord<i32>, Coord<i32>),
        winding: impl FnMut(usize, usize) -> i64,
        plan: &RingPlan<'_>,
        piece: &mut PieceBuf<V>,
    ) -> Result<Closed, TileError> {
        let tmp = &mut self.tmp;
        if *plan == RingPlan::PLAIN {
            close_ring::<true, V>(ring, arcs, min, max, winding, plan, tmp, piece)
        } else {
            close_ring::<false, V>(ring, arcs, min, max, winding, plan, tmp, piece)
        }
    }
}

/// The first arc to emit: `1` when the last arc runs on, across the ring's seam, into the first
/// (the last is then extended over it), else `0`.
fn merge_seam(arcs: &mut [(usize, usize)], m: usize) -> usize {
    if let (Some(&(s0, e0)), Some(last)) = (arcs.first(), arcs.len().checked_sub(1))
        && last > 0
        && arcs[last].1 + 1 >= s0 + m
    {
        arcs[last].1 = e0 + m;
        return 1;
    }
    0
}

/// What a frame attempt probes before building anything: the outside vertices it would keep, and the
/// tips it would have to close (each must turn out to be a vertex of the hull of the first).
pub(crate) struct Probe {
    pub(crate) pts: Vec<Coord<i32>>,
    /// Each tip, with its bridge's number in the piece and which end of the excursion it is (`0`
    /// after an arc, `1` before the next).
    pub(crate) tips: Vec<(Coord<i32>, u32, u8)>,
    /// How many bridges the rings probed so far have.
    pub(crate) bridges: usize,
}

impl Probe {
    pub(crate) const fn new() -> Self {
        Self {
            pts: Vec::new(),
            tips: Vec::new(),
            bridges: 0,
        }
    }

    pub(crate) fn clear(&mut self) {
        self.pts.clear();
        self.tips.clear();
        self.bridges = 0;
    }
}

/// Push onto `probe` the vertices [`close_ring`] keeps of a ring (its arcs, and `deepen` vertices past
/// each tip) that lie outside the box `[min, max]`, without building the ring: the ones that can be
/// on the hull of the piece's outside geometry.
pub(crate) fn arcs_points<V: Vertex>(
    ring: &[V],
    arcs: &mut [(usize, usize)],
    plan: RingPlan<'_>,
    (min, max): (Coord<i32>, Coord<i32>),
    probe: &mut Probe,
) {
    let m = ring.len();
    let first = merge_seam(arcs, m);
    let arcs = &arcs[first..];
    let Probe { pts, tips, bridges } = probe;
    let base = *bridges;
    let mut add = |i: usize| {
        let p = ring[i % m].position();
        if outcode(p, min, max) != 0 {
            pts.push(p);
        }
    };
    if let [(s, e)] = *arcs
        && e - s + 1 >= m
    {
        (0..m).for_each(&mut add); // every vertex is kept
        return;
    }
    for (j, &(s, e)) in arcs.iter().enumerate() {
        (s..=e).for_each(&mut add);
        let next = arcs.get(j + 1).map_or(arcs[0].0 + m, |a| a.0);
        let gap = next - e - 1;
        let (head, tail) = plan.depth(plan.bridge_base + j, gap);
        (e + 1..=e + head).for_each(&mut add);
        (next - tail..next).for_each(&mut add);
        if head + tail < gap {
            let bridge = u32::try_from(plan.bridge_base + j).unwrap_or(u32::MAX);
            tips.push((ring[(e + head) % m].position(), bridge, 0));
            tips.push((ring[(next - tail) % m].position(), bridge, 1));
        }
    }
    *bridges = base + arcs.len();
}

/// Reusable buffers of the frame construction.
struct FrameTmp {
    pts: Vec<Coord<i32>>,
    expanded: Vec<Coord<i32>>,
    /// The lane `expanded` was grown by, for the current attempt's hull.
    grown_by: Option<i32>,
}

impl FrameTmp {
    const fn new() -> Self {
        Self {
            pts: Vec::new(),
            expanded: Vec::new(),
            grown_by: None,
        }
    }
}

/// A tile's piece as it is built: its vertices, where each ring ends, and which of its edges are
/// synthetic (the connectors that close the rings, and the sides of a fill box). Synthetic geometry
/// is only ever written with its marks, so the marks are exactly the synthetic edges, in increasing
/// order, as [`is_planar`](crate::planar::is_planar) looks them up.
pub(crate) struct PieceBuf<V> {
    verts: Vec<V>,
    /// Where each ring ends in `verts`. Every ring is closed: its first vertex repeated.
    ring_ends: Vec<u32>,
    /// The synthetic edges, increasing: edge `i` runs from vertex `i` to vertex `i + 1`. Only these
    /// can cross anything, so only these are checked.
    synthetic: Vec<u32>,
}

impl<V> PieceBuf<V> {
    pub(crate) const fn new() -> Self {
        Self {
            verts: Vec::new(),
            ring_ends: Vec::new(),
            synthetic: Vec::new(),
        }
    }

    /// A piece of `verts`, its rings ending at `ring_ends`, with the `synthetic` edges marked as
    /// given, for testing what reads a piece.
    #[cfg(test)]
    pub(crate) fn from_parts(verts: Vec<V>, ring_ends: Vec<u32>, synthetic: Vec<u32>) -> Self {
        Self {
            verts,
            ring_ends,
            synthetic,
        }
    }

    /// Start a new piece.
    #[inline]
    pub(crate) fn clear(&mut self) {
        self.verts.clear();
        self.ring_ends.clear();
        self.synthetic.clear();
    }

    /// The piece's vertices, each ring closed.
    pub(crate) fn verts(&self) -> &[V] {
        &self.verts
    }

    /// Where each ring ends in [`Self::verts`].
    pub(crate) fn ring_ends(&self) -> &[u32] {
        &self.ring_ends
    }

    /// The synthetic edges, increasing: edge `i` runs from vertex `i` to vertex `i + 1`.
    pub(crate) fn synthetic(&self) -> &[u32] {
        &self.synthetic
    }

    /// How many vertices are written.
    pub(crate) fn len(&self) -> usize {
        self.verts.len()
    }

    /// Make room for `additional` more vertices.
    #[inline]
    pub(crate) fn reserve(&mut self, additional: usize) {
        self.verts.reserve(additional);
    }
}

impl<V: PolyVertex> PieceBuf<V> {
    /// Original vertices, never marked.
    #[inline]
    pub(crate) fn extend_input(&mut self, vs: impl IntoIterator<Item = V>) {
        self.verts.extend(vs);
    }

    /// A ring kept whole: its distinct vertices `ring`, closed, ending its ring.
    #[inline]
    pub(crate) fn push_whole_ring(&mut self, ring: &[V]) -> Result<(), TileError> {
        self.verts.extend_from_slice(ring);
        self.end_ring()
    }

    /// A synthetic path from the last vertex written through `corners` to the next vertex written:
    /// marks every edge of it, the plain chord (no corners) included.
    #[inline]
    pub(crate) fn push_connector(
        &mut self,
        corners: impl IntoIterator<Item = Result<Coord<i32>, TileError>>,
    ) -> Result<(), TileError> {
        let from = self.verts.len() - 1;
        for c in corners {
            self.verts.push(V::synthetic_at(c?));
        }
        self.mark(from, self.verts.len() - from);
        Ok(())
    }

    /// The `B⁺` fill box of the box `[min, max]` as a ring oriented like `orient` (a solid tile
    /// fill), all synthetic, ending its ring.
    pub(crate) fn push_fill_box(
        &mut self,
        orient: Ordering,
        min: Coord<i32>,
        max: Coord<i32>,
    ) -> Result<(), TileError> {
        let ring = fill_box(orient, corner(0, min, max)?, corner(4, min, max)?);
        self.mark(self.verts.len(), 4);
        self.verts.extend(ring[..4].iter().copied().map(V::synthetic_at));
        self.end_ring()
    }

    /// The tile is covered by the exterior: grow its fill box (the piece's first ring) to enclose
    /// the vertices of the rings after it too, one unit clear of them, so no hole reaches outside
    /// its exterior. The ring keeps its length, so its marks still fit it.
    pub(crate) fn grow_fill_box(&mut self, min: Coord<i32>, max: Coord<i32>) -> Result<(), TileError> {
        let first_hole = self.ring_ends[0] as usize;
        let (mut lo, mut hi) = (min, max);
        for v in &self.verts[first_hole..] {
            let p = v.position();
            lo = Coord {
                x: lo.x.min(p.x),
                y: lo.y.min(p.y),
            };
            hi = Coord {
                x: hi.x.max(p.x),
                y: hi.y.max(p.y),
            };
        }
        let (lo, hi) = lane_bounds(lo, hi, 1)?;
        // The grown box is wound like the fill box it replaces (like the exterior).
        let ring = fill_box(ring_orientation(&self.verts[..first_hole]), lo, hi);
        debug_assert_eq!(ring.len(), first_hole, "the fill box is the piece's first ring");
        for (slot, c) in self.verts.iter_mut().zip(ring) {
            *slot = V::synthetic_at(c);
        }
        Ok(())
    }

    /// Close the current ring, repeating its first vertex, and end it.
    #[inline]
    fn end_ring(&mut self) -> Result<(), TileError> {
        let start = self.ring_ends.last().map_or(0, |&e| e as usize);
        self.verts.push(self.verts[start]);
        let end = u32::try_from(self.verts.len()).map_err(|_too_large| TileError::GeometryTooLarge)?;
        self.ring_ends.push(end);
        Ok(())
    }

    /// Mark the `count` edges from vertex `first`, all after the ones marked so far.
    #[inline]
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a piece's vertex count is checked to fit u32 as its rings end"
    )]
    fn mark(&mut self, first: usize, count: usize) {
        debug_assert!(
            self.synthetic.last().is_none_or(|&l| (l as usize) < first),
            "marks are written in increasing order"
        );
        self.synthetic.extend((first..first + count).map(|i| i as u32));
    }
}

/// The strict convex hull of `pts` (sorted and deduplicated in place), counter-clockwise, into `hull`.
pub(crate) fn convex_hull(pts: &mut Vec<Coord<i32>>, hull: &mut Vec<Coord<i32>>) {
    hull.clear();
    pts.sort_unstable_by_key(|c| (c.x, c.y));
    pts.dedup();
    if pts.len() < 3 {
        hull.extend_from_slice(pts);
        return;
    }
    // Andrew's monotone chain: the lower hull left to right, then the upper one back.
    for pass in 0..2 {
        let from = hull.len();
        let walk: &mut dyn Iterator<Item = &Coord<i32>> = if pass == 0 {
            &mut pts.iter()
        } else {
            &mut pts.iter().rev()
        };
        for &p in walk {
            while hull.len() >= from + 2 && orient(hull[hull.len() - 2], hull[hull.len() - 1], p) != Ordering::Greater {
                hull.pop();
            }
            hull.push(p);
        }
        hull.pop(); // the end point starts the next pass
    }
}

/// Close the excursion from tip `u` to tip `v` (winding `w_exc` around the box center) on the hull
/// `hull` grown by `lane` units: each tip steps out to the vertex of the grown hull that it
/// generates, and the grown hull's vertices between them follow, whichever way round winds like the
/// excursion. Writes them as a connector and returns `true`, or writes nothing and returns `false`
/// if a tip is not a hull vertex or neither way round has the excursion's winding.
#[expect(
    clippy::too_many_arguments,
    clippy::many_single_char_names,
    reason = "u and v are the excursion's tips; the rest is the box, hull, lane and buffers it needs"
)]
fn frame_bridge<V: PolyVertex>(
    u: Coord<i32>,
    v: Coord<i32>,
    w_exc: i64,
    (min, max): (Coord<i32>, Coord<i32>),
    hull: &[Coord<i32>],
    lane: i32,
    tmp: &mut FrameTmp,
    piece: &mut PieceBuf<V>,
) -> Result<bool, TileError> {
    let lane = lane.max(1);
    // The hull grown by `lane` (its Minkowski sum with the square `[-lane, lane]²`), kept for the
    // other excursions of the attempt that close in the same lane.
    let mut grown = core::mem::take(&mut tmp.expanded);
    if tmp.grown_by != Some(lane) {
        tmp.pts.clear();
        let g = |a: i32, d: i32| a.checked_add(d).ok_or(TileError::Overflow);
        for c in hull {
            for (sx, sy) in [(-lane, -lane), (lane, -lane), (lane, lane), (-lane, lane)] {
                tmp.pts.push(Coord {
                    x: g(c.x, sx)?,
                    y: g(c.y, sy)?,
                });
            }
        }
        convex_hull(&mut tmp.pts, &mut grown);
        tmp.grown_by = Some(lane);
    }
    let result = (|| {
        // The grown vertex a tip generates: the tip plus a corner of the lane square.
        let generated = |t: Coord<i32>| {
            grown
                .iter()
                .position(|e| e.x.abs_diff(t.x) == lane.unsigned_abs() && e.y.abs_diff(t.y) == lane.unsigned_abs())
        };
        let (Some(i), Some(j)) = (generated(u), generated(v)) else {
            return Ok(false);
        };
        let n = grown.len();
        let cy = center_y(min, max);
        // Both ways round, each as the grown vertices from `i` to `j` (a tip's own vertex first).
        let ccw = (0..=(j + n - i) % n).map(|k| grown[(i + k) % n]);
        let cw = (0..=(i + n - j) % n).map(|k| grown[(i + n - k) % n]);
        let wind = |way: &mut dyn Iterator<Item = Coord<i32>>| {
            ray_crossings(iter::once(u).chain(way).chain(iter::once(v)), cy, max.x)
        };
        let forward = if wind(&mut ccw.clone()) == w_exc {
            true
        } else if wind(&mut cw.clone()) == w_exc {
            false
        } else {
            return Ok(false);
        };
        let way: &mut dyn Iterator<Item = Coord<i32>> = if forward { &mut { ccw } } else { &mut { cw } };
        piece.push_connector(way.map(Ok))?;
        Ok::<_, TileError>(true)
    })();
    tmp.expanded = grown;
    result
}

/// Whether [`close_ring`] closed its ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Closed {
    Ring,
    /// A frame can't close an excursion (a tip is not on the hull, or neither way round winds like
    /// it): the ring is left unfinished, and the attempt fails.
    FrameMissed,
}

/// Close one ring's kept arcs into a single closed ring (first vertex repeated at the end), written
/// to `piece` in the ring's own coordinate frame, ending its ring.
///
/// `ring` holds the ring's distinct vertices in cyclic order (see [`push_distinct`]); edge `i` runs
/// `ring[i] -> ring[(i + 1) % m]`. `arcs` holds, non-empty, the kept arcs that [`keep_edge`] built from
/// the edges touching the box `[min, max]` in ascending order - a vertex is kept iff an incident edge
/// touches; they are consumed. `winding(first, count)` must return
/// [`ray_crossing`] summed over the `count` consecutive edges from edge `first` (cyclically), for this
/// box's ray; the caller supplies it so the single-tile clip can walk the edges while the all-tiles
/// slicer answers from its per-row crossing index.
///
/// Output starts at the first arc in index order, so it is a pure function of the inputs. Only a
/// `plan` with a frame can leave the ring unfinished ([`Closed::FrameMissed`]).
///
/// # Errors
///
/// - [`TileError::Overflow`] if a synthetic corner can't be placed strictly outside the box.
/// - [`TileError::GeometryTooLarge`] if the piece passes `u32::MAX` vertices.
#[inline]
#[expect(
    clippy::too_many_arguments,
    reason = "the ring, its arcs, the box, the winding, the plan and the buffers: one hot function"
)]
fn close_ring<const PLAIN: bool, V: PolyVertex>(
    ring: &[V],
    arcs: &mut [(usize, usize)],
    min: Coord<i32>,
    max: Coord<i32>,
    mut winding: impl FnMut(usize, usize) -> i64,
    plan: &RingPlan<'_>,
    tmp: &mut FrameTmp,
    piece: &mut PieceBuf<V>,
) -> Result<Closed, TileError> {
    let m = ring.len();
    let first = merge_seam(arcs, m);
    let arcs = &arcs[first..];
    if let [(s, e)] = *arcs
        && e - s + 1 >= m
    {
        // Every vertex is kept: the ring is returned verbatim, re-closed.
        piece.push_whole_ring(ring)?;
        return Ok(Closed::Ring);
    }

    let cy = center_y(min, max);
    for (j, &(s, e)) in arcs.iter().enumerate() {
        piece.extend_input((s..=e).map(|i| ring[i % m]));
        // Bridge the gap to the next arc (cyclically): the dropped vertices `e+1 .. next`.
        let next = arcs.get(j + 1).map_or(arcs[0].0 + m, |a| a.0);
        // The plain clip bridges from the tips; a repair may keep `head` more vertices after this
        // arc's tip and `tail` before the next one's (all, if they meet).
        let (head, tail) = if PLAIN {
            (0, 0)
        } else {
            plan.depth(plan.bridge_base + j, next - e - 1)
        };
        if !PLAIN {
            if head + tail == next - e - 1 {
                piece.extend_input((e + 1..next).map(|i| ring[i % m]));
                continue;
            }
            piece.extend_input((e + 1..=e + head).map(|i| ring[i % m]));
        }
        let (ui, vi) = (e + head, next - tail);
        let (u, v) = (ring[ui % m].position(), ring[vi % m].position());
        let w_exc = winding(ui % m, vi - ui);
        let (cmin, cmax) = if PLAIN {
            (min, max)
        } else {
            let lane = i32::try_from(j)
                .ok()
                .and_then(|j| j.checked_mul(plan.lane_step))
                .and_then(|l| l.checked_add(plan.lane_base))
                .ok_or(TileError::Overflow)?;
            if let Some(hull) = plan.frame {
                let lane = lane.checked_add(1).ok_or(TileError::Overflow)?;
                if !frame_bridge(u, v, w_exc, (min, max), hull, lane, tmp, piece)? {
                    return Ok(Closed::FrameMissed);
                }
                piece.extend_input((vi..next).map(|i| ring[i % m]));
                continue;
            }
            lane_bounds(min, max, lane)?
        };
        let bridge = if PLAIN {
            plan_bridge(u, v, w_exc, vi - ui - 1, min, max, cy)?
        } else {
            plan_bridge_lane(u, v, w_exc, vi - ui - 1, (min, max), (cmin, cmax), cy)?
        };
        match bridge {
            Bridge::Keep => piece.extend_input((ui + 1..vi).map(|i| ring[i % m])),
            Bridge::Detour { start, advance } => {
                let before = piece.len();
                piece.push_connector(corner_slots(start, advance).map(|s| corner(s, cmin, cmax)))?;
                debug_assert_eq!(
                    ray_crossings(
                        iter::once(u)
                            .chain(piece.verts()[before..].iter().map(Vertex::position))
                            .chain(iter::once(v)),
                        cy,
                        max.x
                    ),
                    w_exc,
                    "detour winding must match the excursion it replaces"
                );
            }
        }
        if !PLAIN {
            piece.extend_input((vi..next).map(|i| ring[i % m]));
        }
    }
    piece.end_ring()?;
    Ok(Closed::Ring)
}

/// The outcome of clipping one ring to a tile box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RingClip {
    /// The ring does not appear in this tile (nothing was appended).
    Outside,
    /// The tile lies entirely inside the ring: a solid fill (the synthetic `B⁺` box). For an exterior
    /// ring this means a fully-filled tile; for a hole it means the tile is entirely a hole.
    Covers,
    /// The ring crosses the tile; the clipped closed ring (original vertices + synthetic detours).
    Clipped,
}

/// Clip one closed `ring` to the inclusive buffered box `[min, max]`, keeping original vertices and
/// closing the result with synthetic `B⁺` corners. Writes the closed output ring to `piece` in the
/// input's own coordinate frame (first vertex repeated at the end), ending its ring, and tells apart
/// a whole-tile [`RingClip::Covers`] fill from a partial [`RingClip::Clipped`] crossing (so the
/// caller can drop a tile that a hole entirely covers) and [`RingClip::Outside`] when the ring
/// misses the tile.
///
/// # Errors
///
/// - [`TileError::Overflow`] if the tile sits so close to the `i32` edge that a synthetic corner can't
///   be placed strictly outside the box.
/// - [`TileError::GeometryTooLarge`] if the ring has more than `u32::MAX` distinct vertices, or the
///   piece more than `u32::MAX` vertices.
pub(crate) fn clip_ring<V: PolyVertex>(
    ring: &[V],
    min: Coord<i32>,
    max: Coord<i32>,
    scratch: &mut ClipScratch<V>,
    piece: &mut PieceBuf<V>,
) -> Result<RingClip, TileError> {
    let clip = clip_ring_by::<true, V>(ring, min, max, scratch, &RingPlan::PLAIN, None, piece)?;
    // Only a repair's frame can miss: the plain clip closes every ring it reaches.
    debug_assert!(clip.is_some(), "the plain clip has no frame to miss");
    Ok(clip.unwrap_or(RingClip::Outside))
}

/// [`clip_ring`] for a repair: the ring is closed by `plan`, or, given a `probe`, not built at all:
/// the vertices it would keep outside the box go there. `None` if the plan's frame can't close
/// one of the ring's excursions ([`Closed::FrameMissed`]): the attempt fails.
pub(crate) fn clip_ring_planned<V: PolyVertex>(
    ring: &[V],
    min: Coord<i32>,
    max: Coord<i32>,
    scratch: &mut ClipScratch<V>,
    plan: &RingPlan<'_>,
    probe: Option<&mut Probe>,
    piece: &mut PieceBuf<V>,
) -> Result<Option<RingClip>, TileError> {
    clip_ring_by::<false, V>(ring, min, max, scratch, plan, probe, piece)
}

/// [`clip_ring`] (`PLAIN`, ignoring `plan` and `probe`) and [`clip_ring_planned`] in one body, so the
/// plain clip, the hot one, takes no more arguments than it needs.
#[inline(always)]
#[expect(
    clippy::inline_always,
    reason = "each caller is one instance; passing the plan's arguments costs the plain clip ~3%"
)]
fn clip_ring_by<const PLAIN: bool, V: PolyVertex>(
    ring: &[V],
    min: Coord<i32>,
    max: Coord<i32>,
    scratch: &mut ClipScratch<V>,
    plan: &RingPlan<'_>,
    probe: Option<&mut Probe>,
    piece: &mut PieceBuf<V>,
) -> Result<Option<RingClip>, TileError> {
    let ClipScratch {
        copy,
        crossings,
        arcs,
        tmp,
    } = scratch;
    // Borrow the ring when it is already distinct (the usual case), copying only to drop duplicates.
    let pts = if let Some(n) = distinct_prefix(ring) {
        &ring[..n]
    } else {
        copy.clear();
        push_distinct(ring, copy);
        copy.as_slice()
    };
    let m = pts.len();
    if m < 3 {
        return Ok(Some(RingClip::Outside));
    }
    u32::try_from(m).map_err(|_too_large| TileError::GeometryTooLarge)?;
    if all_inside(pts, min, max) {
        // Every vertex is inside (most rings of small polygons): every edge touches, and the ring is
        // kept verbatim, as `close_ring` would. Reserved exactly: a fresh slicer's buffer needs no regrowth.
        if !PLAIN && probe.is_some() {
            return Ok(Some(RingClip::Clipped)); // nothing outside the box
        }
        piece.reserve(m + 1);
        piece.push_whole_ring(pts)?;
        return Ok(Some(RingClip::Clipped));
    }

    // One pass over the edges. Each vertex's outcode is computed once and shared by its two edges: a
    // shared outside bit rejects an edge (the bounding-box test of `segment_intersects`), an inside
    // endpoint accepts it, and only the rest need the exact line test. The same pass records every
    // edge crossing the winding ray (its endpoints on either side of `y = cy`, the only edges
    // `ray_crossing` can count), so an excursion's winding is a sum over those few, not a walk.
    let cy = center_y(min, max);
    let code = |p: Coord<i32>| outcode(p, min, max) | if p.y > cy { ABOVE } else { 0 };
    arcs.clear();
    crossings.clear();
    let (mut a, mut ca) = (pts[0].position(), code(pts[0].position()));
    for (i, b) in (0..).zip(pts[1..].iter().chain(iter::once(&pts[0]))) {
        let b = b.position();
        let cb = code(b);
        let (oa, ob) = (ca & BOX, cb & BOX);
        if oa & ob == 0 && (oa == 0 || ob == 0 || line_meets_box(a, b, min, max)) {
            keep_edge(arcs, i as usize);
        }
        if (ca ^ cb) & ABOVE != 0 {
            let w = ray_crossing(a, b, cy, max.x);
            if w != 0 {
                crossings.push((i, w));
            }
        }
        (a, ca) = (b, cb);
    }

    if arcs.is_empty() {
        // No edge touches the box -> the tile is uniformly inside or outside the ring, and the box
        // center's `+x` ray meets edges only outside the box - exactly the recorded crossings - so their
        // parity is the even-odd containment of the whole tile.
        let inside = crossings.len() % 2 == 1;
        debug_assert_eq!(
            inside,
            point_in_distinct_ring(min, m, |i| pts[i].position()),
            "ray parity must agree with point-in-ring containment"
        );
        return Ok(Some(if inside {
            if PLAIN || probe.is_none() {
                piece.push_fill_box(ring_orientation(pts), min, max)?;
            }
            RingClip::Covers
        } else {
            RingClip::Outside
        }));
    }

    // Signed crossings among edges `lo..hi` (both within `0..=m`).
    let sum = |lo: usize, hi: usize| -> i64 {
        let i = crossings.partition_point(|c| (c.0 as usize) < lo);
        let j = crossings.partition_point(|c| (c.0 as usize) < hi);
        crossings[i..j].iter().map(|c| c.1).sum()
    };
    let winding = |first: usize, count: usize| {
        let w = if first + count <= m {
            sum(first, first + count)
        } else {
            sum(first, m) + sum(0, first + count - m)
        };
        debug_assert_eq!(
            w,
            ray_crossings((first..=first + count).map(|i| pts[i % m].position()), cy, max.x),
            "indexed winding must match walking the excursion"
        );
        w
    };
    if PLAIN || *plan == RingPlan::PLAIN {
        // (The plain clip has no frame to miss.)
        close_ring::<true, V>(pts, arcs, min, max, winding, &RingPlan::PLAIN, tmp, piece)?;
    } else if let Some(acc) = probe {
        arcs_points(pts, arcs, *plan, (min, max), acc);
    } else if close_ring::<false, V>(pts, arcs, min, max, winding, plan, tmp, piece)? == Closed::FrameMissed {
        return Ok(None);
    }
    Ok(Some(RingClip::Clipped))
}

/// How a kept `ring` (at least 3 distinct vertices, see [`push_distinct`]) meets the box
/// `[min, max]` - what [`clip_ring`] returns, without clipping it: [`RingClip::Clipped`] at the
/// first edge touching the box, else the parity of the box center's ray crossings, as there.
pub(crate) fn ring_meets_box<V: Vertex>(ring: &[V], min: Coord<i32>, max: Coord<i32>) -> RingClip {
    let cy = center_y(min, max);
    // Starting from the closing edge's first vertex (a kept ring is never empty).
    let (mut a, mut inside) = (ring[ring.len() - 1].position(), false);
    for v in ring {
        let b = v.position();
        if segment_intersects(a, b, min, max) {
            return RingClip::Clipped;
        }
        inside ^= ray_crossing(a, b, cy, max.x) != 0;
        a = b;
    }
    if inside { RingClip::Covers } else { RingClip::Outside }
}

/// The length of `ring` without its repeated closing vertices, if that prefix has no consecutive
/// duplicate positions - exactly what [`push_distinct`] would copy, so it can be borrowed instead.
/// `None` when an inner duplicate needs the copy.
pub(crate) fn distinct_prefix<V: Vertex>(ring: &[V]) -> Option<usize> {
    let first = ring.first()?.position();
    let mut n = ring.len();
    while n >= 2 && ring[n - 1].position() == first {
        n -= 1;
    }
    ring[..n]
        .windows(2)
        .all(|w| w[0].position() != w[1].position())
        .then_some(n)
}

/// The kept arcs of `ring` for the box `[min, max]` - what [`clip_ring`] finds - appended to `store`,
/// so a probe can revisit them without scanning the ring again. Returns the ring's distinct length
/// and where its arcs are in `store` (empty when no edge touches the box, or every vertex is
/// inside), or `None` if the ring has repeated vertices, which only a copy can drop.
pub(crate) fn scan_arcs<V: Vertex>(
    ring: &[V],
    min: Coord<i32>,
    max: Coord<i32>,
    store: &mut Vec<(usize, usize)>,
) -> Option<(usize, core::ops::Range<usize>)> {
    let m = distinct_prefix(ring)?;
    let pts = &ring[..m];
    let start = store.len();
    if m < 3 || pts.iter().all(|p| outcode(p.position(), min, max) == 0) {
        return Some((m, start..start));
    }
    let (mut a, mut oa) = (pts[0].position(), outcode(pts[0].position(), min, max));
    for (i, b) in pts[1..].iter().chain(iter::once(&pts[0])).enumerate() {
        let b = b.position();
        let ob = outcode(b, min, max);
        if oa & ob == 0 && (oa == 0 || ob == 0 || line_meets_box(a, b, min, max)) {
            // (`keep_edge`, on this ring's arcs only.)
            match store[start..].last_mut() {
                Some((_, end)) if i <= *end + 1 => *end = i + 1,
                _ => store.push((i, i + 1)),
            }
        }
        (a, oa) = (b, ob);
    }
    Some((m, start..store.len()))
}

#[cfg(test)]
mod tests {
    use geo_types::Coord;

    use super::*;
    use crate::vertex::Measured;

    fn c(x: i32, y: i32) -> Coord<i32> {
        Coord { x, y }
    }

    // Tile (0,0), extent 10, buffer 0 -> inclusive box [0,0]..[9,9].
    const MIN: Coord<i32> = Coord { x: 0, y: 0 };
    const MAX: Coord<i32> = Coord { x: 9, y: 9 };

    /// [`clip_ring`] into a fresh output, with how the ring met the tile.
    fn clip(ring: &[Coord<i32>]) -> (RingClip, Vec<Coord<i32>>) {
        let mut piece = PieceBuf::new();
        let rc = clip_ring(ring, MIN, MAX, &mut ClipScratch::new(), &mut piece).unwrap();
        (rc, piece.verts().to_vec())
    }

    #[test]
    fn ring_of_two_distinct_vertices_is_outside() {
        // Distinct only after dropping consecutive duplicates, and then just a line there and back.
        let ring = [c(2, 2), c(2, 2), c(7, 7), c(7, 7), c(2, 2)];
        assert!(matches!(clip(&ring), (RingClip::Outside, out) if out.is_empty()));
    }

    #[test]
    fn hull_is_strict_and_counter_clockwise() {
        let mut pts = vec![c(0, 0), c(4, 0), c(4, 4), c(0, 4), c(2, 2), c(2, 0), c(4, 4)];
        let mut hull = Vec::new();
        convex_hull(&mut pts, &mut hull);
        // The interior point and the collinear one on the bottom edge are dropped.
        assert_eq!(hull, [c(0, 0), c(4, 0), c(4, 4), c(0, 4)]);
        // Fewer than three distinct points come back as they are.
        let mut two = vec![c(1, 1), c(1, 1), c(2, 3)];
        convex_hull(&mut two, &mut hull);
        assert_eq!(hull, [c(1, 1), c(2, 3)]);
    }

    #[test]
    fn marks_are_exactly_the_synthetic_edges() {
        // Input vertices carry 1, synthetic ones the default 0.
        let input = |x, y| Measured::new(x, y, 1_u8);
        let mut piece = PieceBuf::new();
        piece.push_fill_box(Ordering::Less, MIN, MAX).unwrap();
        // A hole: a detour through two corners, then the plain chord back to its first vertex.
        piece.extend_input([input(3, 3), input(3, 12)]);
        piece.push_connector([Ok(c(10, 12)), Ok(c(10, 10))]).unwrap();
        piece.extend_input([input(6, 6), input(12, 3)]);
        piece.push_connector([]).unwrap();
        piece.end_ring().unwrap();
        assert_eq!(piece.ring_ends(), [5, 12]);
        let chord = 10;
        let marked = |piece: &PieceBuf<Measured<u8>>| {
            let v = piece.verts();
            (0..v.len() - 1)
                .filter(|&i| i + 1 != 5) // (no edge joins two rings)
                .filter(|&i| v[i].m == 0 || v[i + 1].m == 0 || i == chord)
                .map(|i| u32::try_from(i).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(piece.synthetic(), marked(&piece));
        assert_eq!(piece.synthetic(), [0, 1, 2, 3, 6, 7, 8, 10]);
        // Grown around the hole, the fill box keeps its length and its marks.
        piece.grow_fill_box(MIN, MAX).unwrap();
        assert_eq!(piece.verts()[0].position, c(-1, -1));
        assert_eq!(piece.verts()[2].position, c(13, 13));
        assert_eq!(piece.ring_ends(), [5, 12]);
        assert_eq!(piece.synthetic(), marked(&piece));
        // A new piece starts with nothing marked.
        piece.clear();
        assert!(piece.verts().is_empty() && piece.synthetic().is_empty());
    }

    #[test]
    fn defensive_edge_cases() {
        assert_eq!(
            slot(0),
            0,
            "an inside point never ends a detour; it maps to a valid slot"
        );
        assert_eq!(ray_crossings(iter::empty(), 0, 0), 0, "an empty path crosses nothing");
    }
}
