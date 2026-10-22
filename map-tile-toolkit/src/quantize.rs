//! Rounding float polygons onto the integer grid without turning a valid polygon into an invalid one.
//!
//! Rounding each vertex on its own moves it up to half a unit per axis.
//! Two edges closer than that can end up crossing, touching, or folding onto each other.
//! For a valid polygon, that can only happen where a vertex was within a unit of another edge.
//! A polygon whose rounded rings are still valid keeps them.
//! Any other polygon is snap-rounded: each float edge bends through the center of every pixel it
//! passes that holds a vertex or an edge crossing, so no two rounded edges cross.
//! The area is then rebuilt from those edges as valid polygons: what the exterior covers, minus what
//! any hole covers.

use core::cmp::Ordering;
use std::collections::HashMap;

use geo_types::Coord;

use crate::TileError;
use crate::geom::{orient, signed_area_2x};
use crate::planar::{Meet, meet};

/// Fractional bits of the fixed-point coordinates a polygon is read into.
const FRAC: u32 = 8;
const ONE: i64 = 1 << FRAC;
const HALF: i64 = ONE / 2;

type Fixed = Coord<i64>;
type Pixel = Coord<i32>;

/// Rounds float polygons onto the integer grid, keeping a valid polygon valid.
///
/// ```
/// use geo_types::coord;
/// use map_tile_toolkit::Quantizer;
///
/// let mut quantizer = Quantizer::new();
/// let exterior = [coord! { x: 0.2, y: 0.1 }, coord! { x: 9.7, y: 0.4 }, coord! { x: 5.1, y: 8.6 }];
/// let rounded = quantizer.polygon([exterior]).unwrap();
/// let polygons: Vec<Vec<&[_]>> = rounded.polygons().map(Iterator::collect).collect();
/// assert_eq!(polygons, [[&[coord! { x: 0, y: 0 }, coord! { x: 10, y: 0 }, coord! { x: 5, y: 9 }][..]]]);
/// ```
#[derive(Debug, Default, Clone)]
pub struct Quantizer {
    fixed: Vec<Fixed>,
    fixed_ends: Vec<usize>,
    rounded_at: Vec<usize>,
    near: Vec<bool>,
    vertices: Vec<Pixel>,
    ring_ends: Vec<usize>,
    polygon_ends: Vec<usize>,
    segs: Vec<Seg>,
    lines: Vec<(Fixed, Fixed)>,
    grid: Grid,
}

/// The polygons [`Quantizer::polygon`] or [`Quantizer::make_valid`] rounded one polygon into.
#[derive(Debug, Clone, Copy)]
pub struct Quantized<'a> {
    q: &'a Quantizer,
}

/// One edge of the rounded rings, with its ring, where that ring starts, and how many edges it has.
#[derive(Debug, Clone, Copy)]
struct Seg {
    a: Pixel,
    b: Pixel,
    ring: usize,
    first: usize,
    edges: usize,
}

/// A rounded edge with how often the exterior and the holes run along it from `a` to `b`.
#[derive(Debug, Clone, Copy)]
struct Net {
    a: Pixel,
    b: Pixel,
    exterior: i32,
    holes: i32,
}

/// How many times the exterior and the holes wind around a point.
#[derive(Debug, Clone, Copy, Default)]
struct Winding {
    exterior: i32,
    holes: i32,
}

impl Winding {
    fn inside(self) -> bool {
        self.exterior != 0 && self.holes == 0
    }
}

impl Quantizer {
    /// An empty quantizer.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Rounds one valid polygon's `rings`, exterior first, in grid units, into valid polygons.
    ///
    /// Only edges that come within a unit of each other are checked.
    /// A polygon that is not valid may come out as invalid as it went in: [`Quantizer::make_valid`]
    /// takes any polygon.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if a coordinate is not finite or rounds outside the `i32` range.
    pub fn polygon<R>(&mut self, rings: R) -> Result<Quantized<'_>, TileError>
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = Coord<f64>>,
    {
        self.read(rings)?;
        debug_assert!(self.read_is_valid(), "Quantizer::polygon was given an invalid polygon");
        self.round();
        if !self.ring_ends.is_empty() {
            if self.rounding_kept_valid() {
                self.keep();
            } else {
                self.repair();
            }
        }
        Ok(Quantized { q: self })
    }

    /// Rounds any polygon's `rings`, exterior first, in grid units, into valid polygons.
    ///
    /// The area kept is what the exterior covers, minus what any hole covers.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if a coordinate is not finite or rounds outside the `i32` range.
    pub fn make_valid<R>(&mut self, rings: R) -> Result<Quantized<'_>, TileError>
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = Coord<f64>>,
    {
        self.read(rings)?;
        self.round();
        if !self.ring_ends.is_empty() {
            if self.rounded_is_valid() {
                self.keep();
            } else {
                self.repair();
            }
        }
        debug_assert!(self.output_is_valid(), "Quantizer::make_valid gave an invalid polygon");
        Ok(Quantized { q: self })
    }

    fn read<R>(&mut self, rings: R) -> Result<(), TileError>
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = Coord<f64>>,
    {
        self.fixed.clear();
        self.fixed_ends.clear();
        for (i, ring) in rings.into_iter().enumerate() {
            let start = self.fixed.len();
            for c in ring {
                let c = Coord {
                    x: to_fixed(c.x)?,
                    y: to_fixed(c.y)?,
                };
                if self.fixed.len() == start || self.fixed.last() != Some(&c) {
                    self.fixed.push(c);
                }
            }
            if self.fixed.len() > start + 1 && self.fixed.last() == self.fixed.get(start) {
                self.fixed.pop();
            }
            if self.fixed.len() - start < 3 {
                self.fixed.truncate(start);
                if i == 0 {
                    return Ok(());
                }
                continue;
            }
            let area = fixed_area_2x(&self.fixed[start..]);
            if (i == 0 && area < 0) || (i > 0 && area > 0) {
                self.fixed[start..].reverse();
            }
            self.fixed_ends.push(self.fixed.len());
        }
        Ok(())
    }

    fn read_is_valid(&self) -> bool {
        let polygon_ends = [self.fixed_ends.len()];
        let polygon_ends = if self.fixed_ends.is_empty() {
            &[][..]
        } else {
            &polygon_ends
        };
        is_valid(&self.fixed, &self.fixed_ends, polygon_ends)
    }

    fn output_is_valid(&self) -> bool {
        let vertices: Vec<Fixed> = self.vertices.iter().map(|&p| widen(p)).collect();
        is_valid(&vertices, &self.ring_ends, &self.polygon_ends)
    }

    /// Rounds every vertex on its own, dropping the rings left with fewer than three, and all of them
    /// when that is the exterior.
    fn round(&mut self) {
        self.vertices.clear();
        self.ring_ends.clear();
        self.polygon_ends.clear();
        self.rounded_at.clear();
        for (i, ring) in rings_of(&self.fixed, &self.fixed_ends).enumerate() {
            let start = self.vertices.len();
            let from = self.rounded_at.len();
            for &c in ring {
                let p = pixel(c);
                if self.vertices.len() == start || self.vertices.last() != Some(&p) {
                    self.vertices.push(p);
                }
                self.rounded_at.push(self.vertices.len() - 1);
            }
            if self.vertices.len() > start + 1 && self.vertices.last() == self.vertices.get(start) {
                self.vertices.pop();
                let last = self.vertices.len();
                for r in &mut self.rounded_at[from..] {
                    if *r == last {
                        *r = start;
                    }
                }
            }
            if self.vertices.len() - start < 3 {
                self.vertices.truncate(start);
                if i == 0 {
                    return;
                }
                self.rounded_at[from..].fill(usize::MAX);
                continue;
            }
            self.ring_ends.push(self.vertices.len());
        }
    }

    /// Keeps the rounded rings as one polygon, the exterior turning positive and the holes negative.
    fn keep(&mut self) {
        let mut start = 0;
        for (i, &end) in self.ring_ends.iter().enumerate() {
            let ring = &mut self.vertices[start..end];
            if (signed_area_2x(ring) > 0) != (i == 0) {
                ring.reverse();
            }
            start = end;
        }
        self.polygon_ends.push(self.ring_ends.len());
    }

    /// Lists the rounded edges, and tells whether no ring folds back onto itself at a vertex.
    fn rounded_edges(&mut self) -> bool {
        self.segs.clear();
        let mut first = 0;
        for (ring, &end) in self.ring_ends.iter().enumerate() {
            let verts = &self.vertices[first..end];
            let edges = verts.len();
            for (k, &at) in verts.iter().enumerate() {
                let (prev, next) = (verts[(k + edges - 1) % edges], verts[(k + 1) % edges]);
                let (back, ahead) = (widen(prev) - widen(at), widen(next) - widen(at));
                if orient(prev, at, next) == Ordering::Equal && back.x * ahead.x + back.y * ahead.y > 0 {
                    return false;
                }
                self.segs.push(Seg {
                    a: at,
                    b: next,
                    ring,
                    first,
                    edges,
                });
            }
            first = end;
        }
        true
    }

    /// Whether rounding kept a valid polygon valid.
    ///
    /// Rounding moves every point by less than a unit along each axis.
    /// Two edges can only come to meet if a vertex of one was within a unit of the other.
    /// A ring can only move across another if all of its vertices were within a unit of it.
    fn rounding_kept_valid(&mut self) -> bool {
        if !self.rounded_edges() {
            return false;
        }
        self.near.clear();
        self.near.resize(self.fixed.len(), false);
        let Self {
            fixed,
            fixed_ends,
            rounded_at,
            near,
            segs,
            grid,
            ..
        } = self;
        let segs = &*segs;
        let incident = |r: usize| {
            let s = segs[r];
            [if r == s.first { s.first + s.edges - 1 } else { r - 1 }, r]
        };
        let apart = |x: usize, y: usize| {
            let (p, q) = (segs[x], segs[y]);
            x == y || adjacent(segs, x, y) || matches!(meet(p.a, p.b, q.a, q.b), Meet::Apart)
        };
        let planar = near_vertices(fixed, fixed_ends, grid, |v, a, b| {
            let (rv, ra, rb) = (rounded_at[v], rounded_at[a], rounded_at[b]);
            if rv == usize::MAX || ra == usize::MAX {
                return true;
            }
            if segs[rv].ring != segs[ra].ring {
                near[v] = true;
            }
            let image = if ra == rb { incident(ra) } else { [ra, ra] };
            incident(rv).iter().all(|&x| image.iter().all(|&y| apart(x, y)))
        });
        planar
            && rings_of(rounded_at, fixed_ends)
                .zip(rings_of(near, fixed_ends))
                .all(|(r, near)| r[0] == usize::MAX || near.contains(&false))
    }

    /// Whether the rounded rings meet only where consecutive edges share a vertex, with every hole
    /// inside the exterior and outside the other holes.
    fn rounded_is_valid(&mut self) -> bool {
        if !self.rounded_edges() {
            return false;
        }
        let segs = &self.segs;
        self.lines.clear();
        self.lines.extend(segs.iter().map(|s| (widen(s.a), widen(s.b))));
        let planar = near_pairs(&self.lines, &mut self.grid, |i, j| {
            let (p, q) = (segs[i], segs[j]);
            adjacent(segs, i, j) || matches!(meet(p.a, p.b, q.a, q.b), Meet::Apart)
        });
        if !planar {
            return false;
        }
        if self.ring_ends.len() == 1 {
            return true;
        }
        let edges: Vec<(Fixed, Fixed)> = self.lines.iter().map(|&(a, b)| left_first(a, b)).collect();
        let hole_starts = &self.ring_ends[..self.ring_ends.len() - 1];
        let queries: Vec<Fixed> = hole_starts.iter().map(|&s| doubled(self.vertices[s])).collect();
        let mut hits = Vec::new();
        rays_up(&edges, &queries, |q, e| {
            if segs[e].ring != q + 1 {
                hits.push((q, segs[e].ring));
            }
        });
        let around = odd_runs(hits);
        around.len() == queries.len()
            && around
                .iter()
                .enumerate()
                .all(|(q, &(hole, ring))| hole == q && ring == 0)
    }

    /// Snap-rounds the polygon and rebuilds its area as valid polygons.
    fn repair(&mut self) {
        self.vertices.clear();
        self.ring_ends.clear();
        self.polygon_ends.clear();
        let mut segs: Vec<(Fixed, Fixed, bool)> = Vec::new();
        for (i, ring) in rings_of(&self.fixed, &self.fixed_ends).enumerate() {
            for (k, &a) in ring.iter().enumerate() {
                segs.push((a, ring[(k + 1) % ring.len()], i > 0));
            }
        }
        let hot = hot_pixels(&segs);
        let net = net_edges(&segs, &hot);
        let rings = trace(boundary(&net));
        self.assemble(rings);
    }

    /// Groups traced rings into polygons: each hole goes to the smallest exterior around it.
    fn assemble(&mut self, rings: Vec<Vec<Pixel>>) {
        let (shells, holes): (Vec<_>, Vec<_>) = rings.into_iter().partition(|r| signed_area_2x(r) > 0);
        let mut owner = vec![0; holes.len()];
        if shells.len() > 1 && !holes.is_empty() {
            let mut edges = Vec::new();
            let mut ring_of = Vec::new();
            for (s, shell) in shells.iter().enumerate() {
                for (k, &a) in shell.iter().enumerate() {
                    edges.push(left_first(widen(a), widen(shell[(k + 1) % shell.len()])));
                    ring_of.push(s);
                }
            }
            let queries: Vec<Fixed> = holes.iter().map(|h| widen(h[0]) + widen(h[1])).collect();
            let mut hits = Vec::new();
            rays_up(&edges, &queries, |q, e| hits.push((q, ring_of[e])));
            let areas: Vec<i128> = shells.iter().map(|s| signed_area_2x(s)).collect();
            owner.fill(usize::MAX);
            for (hole, shell) in odd_runs(hits) {
                if owner[hole] == usize::MAX || areas[shell] < areas[owner[hole]] {
                    owner[hole] = shell;
                }
            }
        }
        for (s, shell) in shells.iter().enumerate() {
            self.vertices.extend_from_slice(shell);
            self.ring_ends.push(self.vertices.len());
            for (hole, _) in holes.iter().zip(&owner).filter(|&(_, &o)| o == s) {
                self.vertices.extend_from_slice(hole);
                self.ring_ends.push(self.vertices.len());
            }
            self.polygon_ends.push(self.ring_ends.len());
        }
    }
}

/// Whether two rounded edges follow each other around a ring.
fn adjacent(segs: &[Seg], i: usize, j: usize) -> bool {
    segs[i].ring == segs[j].ring && (i.abs_diff(j) == 1 || i.abs_diff(j) == segs[i].edges - 1)
}

impl<'a> Quantized<'a> {
    /// Whether rounding left no polygon at all.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.q.polygon_ends.is_empty()
    }

    /// Each polygon's rings, exterior first, open: exteriors have a positive [`signed_area_2x`], holes a
    /// negative one.
    ///
    /// [`signed_area_2x`]: crate::signed_area_2x
    pub fn polygons(self) -> impl Iterator<Item = impl Iterator<Item = &'a [Pixel]> + 'a> + 'a {
        let q = self.q;
        let ring_starts = core::iter::once(0).chain(q.polygon_ends.iter().copied());
        ring_starts.zip(&q.polygon_ends).map(move |(first, &last)| {
            (first..last).map(move |r| {
                let start = if r == 0 { 0 } else { q.ring_ends[r - 1] };
                &q.vertices[start..q.ring_ends[r]]
            })
        })
    }
}

fn rings_of<'a, T>(verts: &'a [T], ends: &'a [usize]) -> impl Iterator<Item = &'a [T]> {
    let starts = core::iter::once(0).chain(ends.iter().copied());
    starts.zip(ends).map(|(s, &e)| &verts[s..e])
}

/// `v` in fixed point, if its pixel fits `i32`.
#[expect(clippy::cast_possible_truncation, reason = "range-checked before the cast")]
#[expect(clippy::cast_precision_loss, reason = "ONE and HALF are small powers of two")]
fn to_fixed(v: f64) -> Result<i64, TileError> {
    // Adding and taking away `1.5 · 2^52` rounds below `2^51` to a whole number, ties to even, and
    // leaves anything bigger out of range.
    const ROUND: f64 = 6_755_399_441_055_744.0;
    let f = (v * ONE as f64 + ROUND) - ROUND;
    let lo = f64::from(i32::MIN) * ONE as f64 - HALF as f64;
    let hi = (f64::from(i32::MAX) + 1.0) * ONE as f64 - HALF as f64;
    if f >= lo && f < hi {
        Ok(f as i64)
    } else {
        Err(TileError::Overflow)
    }
}

/// The pixel holding a fixed-point coordinate: the half-open square `[c - ½, c + ½)` around its center.
#[expect(clippy::cast_possible_truncation, reason = "to_fixed keeps every pixel in i32")]
fn pixel(c: Fixed) -> Pixel {
    Coord {
        x: ((c.x + HALF) >> FRAC) as i32,
        y: ((c.y + HALF) >> FRAC) as i32,
    }
}

fn widen(p: Pixel) -> Coord<i64> {
    Coord {
        x: i64::from(p.x),
        y: i64::from(p.y),
    }
}

/// Reusable working memory of [`near_pairs`].
#[derive(Debug, Default, Clone)]
struct Grid {
    cells: Vec<(u64, usize)>,
    lengths: Vec<i64>,
    columns: Vec<usize>,
    points: Vec<(usize, usize)>,
}

/// Calls `test(i, j)` for every two of `lines` that pass through a common cell of a grid sized to
/// their typical length, until it returns `false`; tells whether it never did.
fn near_pairs(lines: &[(Coord<i64>, Coord<i64>)], grid: &mut Grid, mut test: impl FnMut(usize, usize) -> bool) -> bool {
    let Grid { cells, lengths, .. } = grid;
    cells.clear();
    lengths.clear();
    let Some(&(first, _)) = lines.first() else {
        return true;
    };
    let (mut min, mut max) = (first, first);
    for &(a, b) in lines {
        for c in [a, b] {
            (min.x, min.y, max.x, max.y) = (min.x.min(c.x), min.y.min(c.y), max.x.max(c.x), max.y.max(c.y));
        }
        lengths.push((b.x - a.x).abs().max((b.y - a.y).abs()));
    }
    let size = cell_size(lengths, min, max, 1);
    let cell = |v: i64, o: i64| u64::try_from((v - o) / size).unwrap_or(0);
    let rows = cell(max.y, min.y) + 1;
    for (i, &(a, b)) in lines.iter().enumerate() {
        let (a, b) = if a.x <= b.x { (a, b) } else { (b, a) };
        let (lo, hi) = (a.y.min(b.y), a.y.max(b.y));
        let (c0, c1) = (cell(a.x, min.x), cell(b.x, min.x));
        if c0 == c1 {
            for cy in cell(lo, min.y)..=cell(hi, min.y) {
                cells.push((c0 * rows + cy, i));
            }
            continue;
        }
        // Rounding can only widen a column's span, by a unit either side.
        #[expect(clippy::cast_precision_loss, reason = "a unit of error is allowed for")]
        let at = |x: i64| a.y as f64 + (b.y - a.y) as f64 * ((x - a.x) as f64 / (b.x - a.x) as f64);
        let mut x0 = a.x;
        for cx in c0..=c1 {
            let x1 = (min.x + i64::try_from(cx + 1).unwrap_or(i64::MAX).saturating_mul(size)).min(b.x);
            let (left, right) = (at(x0), at(x1));
            #[expect(clippy::cast_possible_truncation, reason = "clamped to the segment's own span")]
            let (y0, y1) = (
                (left.min(right).floor() as i64 - 1).max(lo),
                (left.max(right).ceil() as i64 + 1).min(hi),
            );
            for cy in cell(y0, min.y)..=cell(y1, min.y) {
                cells.push((cx * rows + cy, i));
            }
            x0 = x1;
        }
    }
    cells.sort_unstable_by_key(|c| c.0);
    for group in cells.chunk_by(|p, q| p.0 == q.0) {
        for (k, p) in group.iter().enumerate() {
            for q in &group[k + 1..] {
                if !test(p.1, q.1) {
                    return false;
                }
            }
        }
    }
    true
}

/// The cell size of a grid over the box from `min` to `max`: the median of `lengths`, but at least
/// `floor` and big enough to hold the box in twice as many cells per axis as there are lengths.
fn cell_size(lengths: &mut [i64], min: Fixed, max: Fixed, floor: i64) -> i64 {
    let mid = lengths.len() / 2;
    let median = *lengths.select_nth_unstable(mid).1;
    let n = i64::try_from(lengths.len()).unwrap_or(i64::MAX);
    median
        .max((max.x - min.x).max(max.y - min.y) / n.saturating_mul(2))
        .max(floor)
}

/// Calls `test(v, a, b)` for every vertex `v` within a unit of an edge from vertex `a` to vertex `b`
/// of the rings ending at `ends`, until it returns `false`; tells whether it never did.
fn near_vertices(
    verts: &[Fixed],
    ends: &[usize],
    grid: &mut Grid,
    mut test: impl FnMut(usize, usize, usize) -> bool,
) -> bool {
    let Grid {
        lengths,
        columns,
        points,
        ..
    } = grid;
    lengths.clear();
    columns.clear();
    points.clear();
    let Some(&first) = verts.first() else {
        return true;
    };
    let (mut min, mut max) = (first, first);
    for &c in verts {
        (min.x, min.y, max.x, max.y) = (min.x.min(c.x), min.y.min(c.y), max.x.max(c.x), max.y.max(c.y));
    }
    let starts = core::iter::once(0).chain(ends.iter().copied());
    let edges = || {
        starts
            .clone()
            .zip(ends)
            .flat_map(|(s, &e)| (s..e).map(move |k| (k, if k + 1 == e { s } else { k + 1 })))
    };
    lengths.extend(edges().map(|(a, b)| {
        let d = verts[b] - verts[a];
        d.x.abs().max(d.y.abs())
    }));
    // Cells twice the typical edge, a power of two in size.
    let shift = cell_size(lengths, min, max, ONE)
        .unsigned_abs()
        .next_power_of_two()
        .trailing_zeros()
        + 1;
    let cell = |v: i64, o: i64| usize::try_from((v - o) >> shift).unwrap_or(0);
    let (cols, rows) = (cell(max.x, min.x) + 1, cell(max.y, min.y) + 1);
    let (column, row) = (
        |x: i64| cell(x, min.x).min(cols - 1),
        |y: i64| cell(y, min.y).min(rows - 1),
    );
    // Each column's vertices, by row.
    columns.resize(cols + 1, 0);
    for c in verts {
        columns[column(c.x) + 1] += 1;
    }
    for k in 1..=cols {
        columns[k] += columns[k - 1];
    }
    points.resize(verts.len(), (0, 0));
    for (i, c) in verts.iter().enumerate() {
        let next = &mut columns[column(c.x)];
        points[*next] = (row(c.y), i);
        *next += 1;
    }
    columns.pop();
    columns.insert(0, 0);
    for k in 0..cols {
        points[columns[k]..columns[k + 1]].sort_unstable_by_key(|p| p.0);
    }
    for (a, b) in edges() {
        let (p, q) = (verts[a], verts[b]);
        let (l, r) = if p.x <= q.x { (p, q) } else { (q, p) };
        let (lo, hi) = (l.y.min(r.y), l.y.max(r.y));
        #[expect(clippy::cast_precision_loss, reason = "a unit of error is allowed for")]
        let at = |x: i64| l.y as f64 + (r.y - l.y) as f64 * ((x - l.x) as f64 / (r.x - l.x) as f64);
        let (c0, c1) = (column(l.x - ONE), column(r.x + ONE));
        for cx in c0..=c1 {
            // An edge across a few columns is probed over all of its height.
            #[expect(clippy::cast_possible_truncation, reason = "clamped to the edge's own span")]
            let (y0, y1) = if c1 - c0 <= 2 {
                (lo, hi)
            } else {
                let x0 = min.x + (i64::try_from(cx).unwrap_or(i64::MAX) << shift);
                let (xa, xb) = ((x0 - ONE).clamp(l.x, r.x), (x0 + (1 << shift) + ONE).clamp(l.x, r.x));
                let (ya, yb) = (at(xa), at(xb));
                ((ya.min(yb) as i64 - 2).max(lo), (ya.max(yb) as i64 + 2).min(hi))
            };
            let (r0, r1) = (row(y0 - ONE), row(y1 + ONE));
            let col = &points[columns[cx]..columns[cx + 1]];
            for &(y, v) in &col[col.partition_point(|p| p.0 < r0)..] {
                if y > r1 {
                    break;
                }
                if v != a && v != b && within(p, q, verts[v]) && !test(v, a, b) {
                    return false;
                }
            }
        }
    }
    true
}

/// Whether the edge `a`-`b` passes within a unit of `v` along both axes.
fn within(a: Fixed, b: Fixed, v: Fixed) -> bool {
    if a.x.min(b.x) > v.x + ONE || a.x.max(b.x) < v.x - ONE || a.y.min(b.y) > v.y + ONE || a.y.max(b.y) < v.y - ONE {
        return false;
    }
    let d = b - a;
    cross(d, v - a).abs() <= i128::from(ONE) * i128::from(d.x.abs() + d.y.abs())
}

/// How two edges meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contact {
    Apart,
    /// At one point only.
    Point,
    /// Crossing, or along a stretch.
    Bad,
}

fn contact((p1, p2): (Fixed, Fixed), (q1, q2): (Fixed, Fixed)) -> Contact {
    let boxed = |c: Fixed, a: Fixed, b: Fixed| {
        c.x >= a.x.min(b.x) && c.x <= a.x.max(b.x) && c.y >= a.y.min(b.y) && c.y <= a.y.max(b.y)
    };
    let side = |a: Fixed, b: Fixed, c: Fixed| cross(b - a, c - a).signum();
    let (s1, s2) = (side(p1, p2, q1), side(p1, p2, q2));
    let (s3, s4) = (side(q1, q2, p1), side(q1, q2, p2));
    if s1 * s2 < 0 && s3 * s4 < 0 {
        return Contact::Bad;
    }
    if s1 == 0 && s2 == 0 {
        let key = |c: Fixed| if p1.x == p2.x { c.y } else { c.x };
        let lo = key(p1).min(key(p2)).max(key(q1).min(key(q2)));
        let hi = key(p1).max(key(p2)).min(key(q1).max(key(q2)));
        return match lo.cmp(&hi) {
            Ordering::Less => Contact::Bad,
            Ordering::Equal => Contact::Point,
            Ordering::Greater => Contact::Apart,
        };
    }
    let touches = (s1 == 0 && boxed(q1, p1, p2))
        || (s2 == 0 && boxed(q2, p1, p2))
        || (s3 == 0 && boxed(p1, q1, q2))
        || (s4 == 0 && boxed(p2, q1, q2));
    if touches { Contact::Point } else { Contact::Apart }
}

/// Whether the polygons' rings are simple, cross no other ring, and nest as exteriors and holes:
/// what an OGC-valid multipolygon needs, short of a connected interior.
fn is_valid(verts: &[Fixed], ring_ends: &[usize], polygon_ends: &[usize]) -> bool {
    let mut lines = Vec::new();
    let mut ring_of = Vec::new();
    for (r, ring) in rings_of(verts, ring_ends).enumerate() {
        let n = ring.len();
        if n < 3 {
            return false;
        }
        for k in 0..n {
            let (prev, at, next) = (ring[(k + n - 1) % n], ring[k], ring[(k + 1) % n]);
            let (back, ahead) = (prev - at, next - at);
            if cross(back, ahead) == 0
                && i128::from(back.x) * i128::from(ahead.x) + i128::from(back.y) * i128::from(ahead.y) > 0
            {
                return false;
            }
            lines.push((at, next));
            ring_of.push(r);
        }
    }
    let ring_len = |r: usize| ring_ends[r] - if r == 0 { 0 } else { ring_ends[r - 1] };
    let simple = near_pairs(&lines, &mut Grid::default(), |i, j| match contact(lines[i], lines[j]) {
        Contact::Apart => true,
        Contact::Point => ring_of[i] != ring_of[j] || i.abs_diff(j) == 1 || i.abs_diff(j) == ring_len(ring_of[i]) - 1,
        Contact::Bad => false,
    });
    if !simple {
        return false;
    }
    let mut polygon_of = Vec::new();
    let mut shell_of = Vec::new();
    let starts = core::iter::once(0).chain(polygon_ends.iter().copied());
    for (p, (first, &last)) in starts.zip(polygon_ends).enumerate() {
        polygon_of.extend(core::iter::repeat_n(p, last - first));
        shell_of.extend(core::iter::repeat_n(first, last - first));
    }
    let edges: Vec<(Fixed, Fixed)> = lines.iter().map(|&(a, b)| left_first(a, b)).collect();
    let queries: Vec<Fixed> = rings_of(verts, ring_ends).map(|r| r[0] + r[1]).collect();
    let mut hits = Vec::new();
    rays_up(&edges, &queries, |q, e| {
        if ring_of[e] != q {
            hits.push((q, ring_of[e]));
        }
    });
    let around = odd_runs(hits);
    let mut held = vec![false; queries.len()];
    for group in around.chunk_by(|a, b| a.0 == b.0 && polygon_of[a.1] == polygon_of[b.1]) {
        let (q, r) = group[0];
        if polygon_of[r] != polygon_of[q] {
            if group.len() != 2 || shell_of[r] != r {
                return false;
            }
        } else if shell_of[q] == q || group != [(q, shell_of[q])] {
            return false;
        } else {
            held[q] = true;
        }
    }
    (0..queries.len()).all(|q| shell_of[q] == q || held[q])
}

fn doubled(p: Pixel) -> Fixed {
    Coord {
        x: 2 * i64::from(p.x),
        y: 2 * i64::from(p.y),
    }
}

fn left_first(a: Fixed, b: Fixed) -> (Fixed, Fixed) {
    if (a.x, a.y) <= (b.x, b.y) { (a, b) } else { (b, a) }
}

fn cross(a: Fixed, b: Fixed) -> i128 {
    i128::from(a.x) * i128::from(b.y) - i128::from(a.y) * i128::from(b.x)
}

fn fixed_area_2x(ring: &[Fixed]) -> i128 {
    let o = ring[0];
    ring.windows(2).map(|w| cross(w[0] - o, w[1] - o)).sum()
}

/// Every pixel holding a vertex or a point where two edges cross, sorted.
fn hot_pixels(segs: &[(Fixed, Fixed, bool)]) -> Vec<Pixel> {
    let mut hot: Vec<Pixel> = segs.iter().map(|s| pixel(s.0)).collect();
    let lines: Vec<(Fixed, Fixed)> = segs.iter().map(|s| (s.0, s.1)).collect();
    near_pairs(&lines, &mut Grid::default(), |i, j| {
        let ((p1, p2), (q1, q2)) = (lines[i], lines[j]);
        if p1 != q1 && p1 != q2 && p2 != q1 && p2 != q2 {
            let (d, e) = (p2 - p1, q2 - q1);
            let (s1, s2) = (cross(d, q1 - p1).signum(), cross(d, q2 - p1).signum());
            let (s3, s4) = (cross(e, p1 - q1).signum(), cross(e, p2 - q1).signum());
            if s1 * s2 < 0 && s3 * s4 < 0 {
                hot.push(crossing(p1, d, q1, e));
            }
        }
        true
    });
    hot.sort_unstable_by_key(|p| (p.x, p.y));
    hot.dedup();
    hot
}

/// The pixel of the point where `p + t·d` crosses `q + s·e`, for two edges known to cross.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the crossing lies on both edges, so in i32"
)]
fn crossing(p: Fixed, d: Fixed, q: Fixed, e: Fixed) -> Pixel {
    let (mut num, mut den) = (cross(q - p, e), cross(d, e));
    if den < 0 {
        (num, den) = (-num, -den);
    }
    let at =
        |p: i64, d: i64| ((i128::from(p + HALF) * den + num * i128::from(d)).div_euclid(den * i128::from(ONE))) as i32;
    Coord {
        x: at(p.x, d.x),
        y: at(p.y, d.y),
    }
}

/// A bound on the parameter `t = n / d` (`d > 0`) of a point along an edge.
#[derive(Clone, Copy)]
struct Bound {
    n: i128,
    d: i128,
    open: bool,
}

impl Bound {
    fn cmp(self, other: Self) -> Ordering {
        (self.n * other.d).cmp(&(other.n * self.d))
    }
}

/// Whether the edge `p`-`q` passes through pixel `h`.
fn passes(p: Fixed, q: Fixed, h: Pixel) -> bool {
    let mut lo = Bound {
        n: 0,
        d: 1,
        open: false,
    };
    let mut hi = Bound {
        n: 1,
        d: 1,
        open: false,
    };
    for (p0, d, c) in [(p.x, q.x - p.x, h.x), (p.y, q.y - p.y, h.y)] {
        let (min, max) = (i64::from(c) * ONE - HALF, i64::from(c) * ONE + HALF);
        let (from, to) = match d.cmp(&0) {
            Ordering::Equal if (min..max).contains(&p0) => continue,
            Ordering::Equal => return false,
            Ordering::Greater => (
                Bound {
                    n: (min - p0).into(),
                    d: d.into(),
                    open: false,
                },
                Bound {
                    n: (max - p0).into(),
                    d: d.into(),
                    open: true,
                },
            ),
            Ordering::Less => (
                Bound {
                    n: (p0 - max).into(),
                    d: (-d).into(),
                    open: true,
                },
                Bound {
                    n: (p0 - min).into(),
                    d: (-d).into(),
                    open: false,
                },
            ),
        };
        match from.cmp(lo) {
            Ordering::Greater => lo = from,
            Ordering::Equal => lo.open |= from.open,
            Ordering::Less => {}
        }
        match to.cmp(hi) {
            Ordering::Less => hi = to,
            Ordering::Equal => hi.open |= to.open,
            Ordering::Greater => {}
        }
    }
    match lo.cmp(hi) {
        Ordering::Less => true,
        Ordering::Equal => !lo.open && !hi.open,
        Ordering::Greater => false,
    }
}

/// Each edge snapped through the hot pixels it passes, summed per rounded edge, keeping those that
/// some ring still runs along.
fn net_edges(segs: &[(Fixed, Fixed, bool)], hot: &[Pixel]) -> Vec<Net> {
    let mut frags = Vec::new();
    let mut path: Vec<(i128, Pixel)> = Vec::new();
    for &(p, q, hole) in segs {
        let (pp, pq) = (pixel(p), pixel(q));
        if pp == pq {
            continue;
        }
        let (lo, hi) = (
            pixel(Coord {
                x: p.x.min(q.x),
                y: p.y.min(q.y),
            }),
            pixel(Coord {
                x: p.x.max(q.x),
                y: p.y.max(q.y),
            }),
        );
        let from = hot.partition_point(|h| h.x < lo.x);
        let to = hot.partition_point(|h| h.x <= hi.x);
        path.clear();
        path.push((i128::MIN, pp));
        for &h in &hot[from..to] {
            if (lo.y..=hi.y).contains(&h.y) && h != pp && h != pq && passes(p, q, h) {
                let c = Coord {
                    x: i64::from(h.x) * ONE,
                    y: i64::from(h.y) * ONE,
                };
                let along =
                    i128::from(c.x - p.x) * i128::from(q.x - p.x) + i128::from(c.y - p.y) * i128::from(q.y - p.y);
                path.push((along, h));
            }
        }
        path.sort_unstable_by_key(|s| s.0);
        path.push((i128::MAX, pq));
        for w in path.windows(2) {
            let (a, b) = (w[0].1, w[1].1);
            let (exterior, holes) = if hole { (0, 1) } else { (1, 0) };
            let (a, b, sign) = if (a.x, a.y) < (b.x, b.y) { (a, b, 1) } else { (b, a, -1) };
            frags.push(Net {
                a,
                b,
                exterior: sign * exterior,
                holes: sign * holes,
            });
        }
    }
    frags.sort_unstable_by_key(|f| (f.a.x, f.a.y, f.b.x, f.b.y));
    let mut net: Vec<Net> = Vec::new();
    for f in frags {
        match net.last_mut() {
            Some(n) if n.a == f.a && n.b == f.b => {
                n.exterior += f.exterior;
                n.holes += f.holes;
            }
            _ => net.push(f),
        }
    }
    net.retain(|n| n.exterior != 0 || n.holes != 0);
    net
}

/// For each query point (in doubled coordinates), calls `hit(query, edge)` for each edge that the
/// upward ray from just right of the point crosses; each edge is given left end first.
fn rays_up(edges: &[(Fixed, Fixed)], queries: &[Fixed], mut hit: impl FnMut(usize, usize)) {
    let x2 = |v: i64| 2 * v;
    let mut order: Vec<usize> = (0..edges.len()).filter(|&e| edges[e].0.x != edges[e].1.x).collect();
    order.sort_unstable_by_key(|&e| edges[e].0.x);
    let mut by_x: Vec<usize> = (0..queries.len()).collect();
    by_x.sort_unstable_by_key(|&q| queries[q].x);
    let mut active: Vec<usize> = Vec::new();
    let mut next = 0;
    for q in by_x {
        let m = queries[q];
        while next < order.len() && x2(edges[order[next]].0.x) <= m.x {
            active.push(order[next]);
            next += 1;
        }
        active.retain(|&e| x2(edges[e].1.x) > m.x);
        for &e in &active {
            let (l, r) = (edges[e].0 * 2, edges[e].1 * 2);
            if cross(r - l, m - l) < 0 {
                hit(q, e);
            }
        }
    }
}

/// The `(query, ring)` pairs that occur an odd number of times: the rings around each query point.
fn odd_runs(mut hits: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    hits.sort_unstable();
    let mut odd: Vec<(usize, usize)> = Vec::new();
    for h in hits {
        if odd.last() == Some(&h) {
            odd.pop();
        } else {
            odd.push(h);
        }
    }
    odd
}

/// The edges between the area and the rest, each directed with the area on its left.
fn boundary(net: &[Net]) -> Vec<(Pixel, Pixel)> {
    let edges: Vec<(Fixed, Fixed)> = net.iter().map(|n| (widen(n.a), widen(n.b))).collect();
    let queries: Vec<Fixed> = edges.iter().map(|&(a, b)| a + b).collect();
    let mut above = vec![Winding::default(); net.len()];
    rays_up(&edges, &queries, |q, e| {
        if q != e {
            above[q].exterior -= net[e].exterior;
            above[q].holes -= net[e].holes;
        }
    });
    let mut half = Vec::new();
    for (n, w) in net.iter().zip(above) {
        // A sloped edge's query reads its upper side, its left; a vertical one reads its right side.
        let (left, right) = if n.a.x == n.b.x {
            (
                Winding {
                    exterior: w.exterior + n.exterior,
                    holes: w.holes + n.holes,
                },
                w,
            )
        } else {
            (
                w,
                Winding {
                    exterior: w.exterior - n.exterior,
                    holes: w.holes - n.holes,
                },
            )
        };
        match (left.inside(), right.inside()) {
            (true, false) => half.push((n.a, n.b)),
            (false, true) => half.push((n.b, n.a)),
            _ => {}
        }
    }
    half
}

/// Which half of the plane a direction points into, counter-clockwise from `+x`.
fn half_plane(d: Coord<i64>) -> u8 {
    u8::from(!(d.y > 0 || (d.y == 0 && d.x > 0)))
}

/// Orders directions counter-clockwise from `+x`.
fn by_angle(a: Coord<i64>, b: Coord<i64>) -> Ordering {
    half_plane(a).cmp(&half_plane(b)).then_with(|| 0.cmp(&cross(a, b)))
}

fn direction(from: Pixel, to: Pixel) -> Coord<i64> {
    Coord {
        x: i64::from(to.x) - i64::from(from.x),
        y: i64::from(to.y) - i64::from(from.y),
    }
}

/// Walks the boundary into simple rings: at each vertex it turns as far left as it can, and a walk
/// that comes back to a vertex splits there.
fn trace(mut half: Vec<(Pixel, Pixel)>) -> Vec<Vec<Pixel>> {
    half.sort_unstable_by(|p, q| {
        (p.0.x, p.0.y)
            .cmp(&(q.0.x, q.0.y))
            .then_with(|| by_angle(direction(p.0, p.1), direction(q.0, q.1)))
    });
    let mut used = vec![false; half.len()];
    let mut rings = Vec::new();
    let mut walk = Vec::new();
    for start in 0..half.len() {
        if used[start] {
            continue;
        }
        walk.clear();
        let mut h = start;
        let closed = loop {
            used[h] = true;
            let (a, v) = half[h];
            walk.push(a);
            let from = half.partition_point(|e| (e.0.x, e.0.y) < (v.x, v.y));
            let to = half.partition_point(|e| (e.0.x, e.0.y) <= (v.x, v.y));
            if from == to {
                break false;
            }
            let back = direction(v, a);
            let past = from + half[from..to].partition_point(|e| by_angle(direction(v, e.1), back) == Ordering::Less);
            h = if past == from { to - 1 } else { past - 1 };
            if h == start {
                break true;
            }
            if used[h] {
                break false;
            }
        };
        if closed {
            split(&walk, &mut rings);
        }
    }
    rings
}

/// Splits a closed walk into simple rings wherever it comes back to a vertex.
fn split(walk: &[Pixel], rings: &mut Vec<Vec<Pixel>>) {
    let mut stack: Vec<Pixel> = Vec::new();
    let mut at: HashMap<(i32, i32), usize> = HashMap::new();
    for &v in walk {
        if let Some(&i) = at.get(&(v.x, v.y)) {
            let ring: Vec<Pixel> = stack.drain(i..).collect();
            for c in &ring[1..] {
                at.remove(&(c.x, c.y));
            }
            stack.push(v);
            if ring.len() >= 3 {
                rings.push(ring);
            }
        } else {
            at.insert((v.x, v.y), stack.len());
            stack.push(v);
        }
    }
    if stack.len() >= 3 {
        rings.push(stack);
    }
}
