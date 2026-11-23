//! Rounding float polygons onto the integer grid without turning a valid polygon into an invalid one.

use core::cmp::Ordering;
use core::ops::{Add, AddAssign, Neg, Range, Sub, SubAssign};
use std::collections::HashMap;
use std::iter::once;

use geo_types::Coord;

use crate::TileError;
use crate::geom::signed_area_2x;

/// Fractional bits of the fixed-point coordinates a polygon is read into.
const FRAC: u32 = 8;
/// One grid unit in fixed point.
const ONE: i64 = 1 << FRAC;
/// Half a grid unit in fixed point.
const HALF: i64 = ONE / 2;

/// A coordinate in fixed point, [`ONE`] to a grid unit.
type Fixed = Coord<i64>;
/// A coordinate rounded to a grid point.
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
    fixed_polygon_ends: Vec<usize>,
    rounded_at: Vec<Option<usize>>,
    near_other_ring: Vec<bool>,
    vertices: Vec<Pixel>,
    ring_ends: Vec<usize>,
    polygon_ends: Vec<usize>,
    segs: Vec<Seg>,
    lines: Vec<(Fixed, Fixed)>,
    grid: Grid,
    groups: PartGroups,
    group: Option<Box<Self>>,
}

/// Reusable working memory that sorts a multipolygon's parts into groups that may round onto each other.
#[derive(Debug, Default, Clone)]
struct PartGroups {
    boxes: Vec<(Pixel, Pixel)>,
    starts: Vec<i32>,
    ends: Vec<i32>,
    roots: Vec<usize>,
    order: Vec<usize>,
    active: Vec<usize>,
    grouped: Vec<(usize, usize)>,
}

impl PartGroups {
    /// Groups the polygons whose rings end at `ring_ends` in `fixed` and that end at `polygon_ends`,
    /// as `(first part, part)` pairs sorted by both: parts are grouped when the pixels their vertices
    /// round into span touching boxes, directly or through other parts.
    ///
    /// Sweeps along the axis on which fewer pairs of boxes overlap, so a column of islands costs no more
    /// than a row of them.
    fn group(&mut self, fixed: &[Fixed], ring_ends: &[usize], polygon_ends: &[usize]) -> &[(usize, usize)] {
        let Self {
            boxes,
            starts,
            ends,
            roots,
            order,
            active,
            grouped,
        } = self;
        boxes.clear();
        for rings in spans(polygon_ends) {
            let vertices = span(ring_ends, rings.start).start..ring_ends[rings.end - 1];
            let (min, max) = bounds(fixed[vertices].iter().copied()).expect("a part keeps its exterior");
            boxes.push((pixel(min), pixel(max)));
        }
        let along_x = sweep_checks(boxes, |(lo, hi)| (lo.x, hi.x), starts, ends);
        let along_y = sweep_checks(boxes, |(lo, hi)| (lo.y, hi.y), starts, ends);
        if along_y < along_x {
            for (lo, hi) in boxes.iter_mut() {
                (*lo, *hi) = (Coord { x: lo.y, y: lo.x }, Coord { x: hi.y, y: hi.x });
            }
        }
        roots.clear();
        roots.extend(0..boxes.len());
        order.clear();
        order.extend(0..boxes.len());
        order.sort_unstable_by_key(|&p| boxes[p].0.x);
        active.clear();
        for &p in &*order {
            let (lo, hi) = boxes[p];
            active.retain(|&q| boxes[q].1.x >= lo.x);
            for &q in &*active {
                if boxes[q].0.y <= hi.y && lo.y <= boxes[q].1.y {
                    let (a, b) = (root(roots, p), root(roots, q));
                    roots[a.max(b)] = a.min(b);
                }
            }
            active.push(p);
        }
        grouped.clear();
        for p in 0..boxes.len() {
            let r = root(roots, p);
            grouped.push((r, p));
        }
        grouped.sort_unstable();
        grouped
    }
}

/// How many pairs of `boxes` a sweep along the axis whose range `axis` gives would check: for each box,
/// those starting no later that do not end before it starts.
fn sweep_checks(
    boxes: &[(Pixel, Pixel)],
    axis: fn(&(Pixel, Pixel)) -> (i32, i32),
    starts: &mut Vec<i32>,
    ends: &mut Vec<i32>,
) -> usize {
    starts.clear();
    ends.clear();
    for b in boxes {
        let (start, end) = axis(b);
        starts.push(start);
        ends.push(end);
    }
    starts.sort_unstable();
    ends.sort_unstable();
    starts
        .iter()
        .enumerate()
        .map(|(i, &start)| i - ends.partition_point(|&end| end < start))
        .sum()
}

/// The root of `p` in the forest `roots`, halving the path to it.
fn root(roots: &mut [usize], mut p: usize) -> usize {
    while roots[p] != p {
        roots[p] = roots[roots[p]];
        p = roots[p];
    }
    p
}

/// The polygons [`Quantizer::polygon`], [`Quantizer::multi_polygon`] or [`Quantizer::make_valid`] rounded into.
#[derive(Debug, Clone, Copy)]
pub struct Quantized<'a> {
    quantizer: &'a Quantizer,
}

/// One edge of the rounded rings, with its ring, where that ring starts, and how many edges it has.
#[derive(Debug, Clone, Copy)]
struct Seg {
    a: Pixel,
    b: Pixel,
    ring: usize,
    ring_start: usize,
    ring_len: usize,
}

impl Seg {
    /// The edge with `i64` coordinates.
    fn line(self) -> (Fixed, Fixed) {
        (widen(self.a), widen(self.b))
    }
}

/// An edge of the fixed-point rings with the winding its ring adds on its left.
#[derive(Debug, Clone, Copy)]
struct FixedEdge {
    a: Fixed,
    b: Fixed,
    winding: Winding,
}

/// A rounded edge with how often the exterior and the holes run along it from `a` to `b`.
#[derive(Debug, Clone, Copy)]
struct NetEdge {
    a: Pixel,
    b: Pixel,
    winding: Winding,
}

/// How many times the exterior and the holes wind around a point.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Winding {
    exterior: i32,
    holes: i32,
}

impl Winding {
    /// One turn of the exterior.
    const EXTERIOR: Self = Self { exterior: 1, holes: 0 };
    /// One turn of a hole.
    const HOLE: Self = Self { exterior: 0, holes: 1 };

    /// Whether the point is inside the exterior and outside every hole.
    fn inside(self) -> bool {
        self.exterior != 0 && self.holes == 0
    }

    /// Whether the point is inside more exteriors than holes: in some part of a valid multipolygon.
    fn in_some_part(self) -> bool {
        self.exterior + self.holes > 0
    }
}

impl Add for Winding {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            exterior: self.exterior + other.exterior,
            holes: self.holes + other.holes,
        }
    }
}

impl Sub for Winding {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        self + -other
    }
}

impl Neg for Winding {
    type Output = Self;

    fn neg(self) -> Self {
        Self {
            exterior: -self.exterior,
            holes: -self.holes,
        }
    }
}

impl AddAssign for Winding {
    fn add_assign(&mut self, other: Self) {
        *self = *self + other;
    }
}

impl SubAssign for Winding {
    fn sub_assign(&mut self, other: Self) {
        *self = *self - other;
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
    /// # Errors
    ///
    /// [`TileError::Overflow`] if a coordinate is not finite or rounds outside the `i32` range.
    pub fn polygon<R>(&mut self, rings: R) -> Result<Quantized<'_>, TileError>
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = Coord<f64>>,
    {
        self.read(once(rings))?;
        self.finish(Self::rounding_kept_valid, Winding::inside);
        debug_assert!(
            self.output_is_valid(),
            "Quantizer::polygon was given an invalid polygon"
        );
        Ok(Quantized { quantizer: self })
    }

    /// Rounds a multipolygon's `polygons`, each its rings, exterior first, in grid units, into valid
    /// polygons.
    ///
    /// Each polygon must be valid on its own, as for [`Self::polygon`], but they may touch or overlap:
    /// parts whose rounded rings meet or overlap merge into one polygon.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if a coordinate is not finite or rounds outside the `i32` range.
    pub fn multi_polygon<P>(&mut self, polygons: P) -> Result<Quantized<'_>, TileError>
    where
        P: IntoIterator,
        P::Item: IntoIterator,
        <P::Item as IntoIterator>::Item: IntoIterator<Item = Coord<f64>>,
    {
        self.read(polygons)?;
        let parts = self.fixed_polygon_ends.len();
        let mut groups = std::mem::take(&mut self.groups);
        let grouped: &[(usize, usize)] = if parts <= 1 {
            &[]
        } else {
            groups.group(&self.fixed, &self.fixed_ends, &self.fixed_polygon_ends)
        };
        if grouped
            .first()
            .is_none_or(|&(first, _)| grouped.iter().all(|g| g.0 == first))
        {
            self.finish_group(parts);
        } else {
            let mut group = self.group.take().unwrap_or_default();
            self.vertices.clear();
            self.ring_ends.clear();
            self.polygon_ends.clear();
            for members in grouped.chunk_by(|a, b| a.0 == b.0) {
                group.fixed.clear();
                group.fixed_ends.clear();
                group.fixed_polygon_ends.clear();
                for &(_, p) in members {
                    for r in span(&self.fixed_polygon_ends, p) {
                        group.fixed.extend_from_slice(&self.fixed[span(&self.fixed_ends, r)]);
                        group.fixed_ends.push(group.fixed.len());
                    }
                    group.fixed_polygon_ends.push(group.fixed_ends.len());
                }
                group.finish_group(members.len());
                let (vertex_offset, ring_offset) = (self.vertices.len(), self.ring_ends.len());
                self.vertices.extend_from_slice(&group.vertices);
                self.ring_ends.extend(group.ring_ends.iter().map(|e| e + vertex_offset));
                self.polygon_ends
                    .extend(group.polygon_ends.iter().map(|e| e + ring_offset));
            }
            self.group = Some(group);
        }
        self.groups = groups;
        debug_assert!(
            self.output_is_valid(),
            "Quantizer::multi_polygon was given a polygon that is not valid on its own"
        );
        Ok(Quantized { quantizer: self })
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
        self.read(once(rings))?;
        self.finish(Self::rounded_is_valid, Winding::inside);
        debug_assert!(self.output_is_valid(), "Quantizer::make_valid gave an invalid polygon");
        Ok(Quantized { quantizer: self })
    }

    /// Rounds the polygons read, keeping the rounded rings if `still_valid` holds and repairing them
    /// into the area where `inside` holds if not.
    fn finish(&mut self, still_valid: fn(&mut Self) -> bool, inside: fn(Winding) -> bool) {
        self.round();
        if !self.ring_ends.is_empty() {
            if still_valid(self) {
                self.keep();
            } else {
                self.repair(inside);
            }
        }
    }

    /// Rounds a group of `parts` polygons read: one as [`Self::polygon`] does, more keeping the rounded
    /// rings only if none of them meet, and their union if any do.
    fn finish_group(&mut self, parts: usize) {
        if parts == 1 {
            self.finish(Self::rounding_kept_valid, Winding::inside);
        } else {
            self.finish(
                |q| q.rounded_rings_apart() && q.output_is_valid(),
                Winding::in_some_part,
            );
        }
    }

    /// Reads each of `polygons` with [`Self::read_polygon`].
    fn read<P>(&mut self, polygons: P) -> Result<(), TileError>
    where
        P: IntoIterator,
        P::Item: IntoIterator,
        <P::Item as IntoIterator>::Item: IntoIterator<Item = Coord<f64>>,
    {
        self.fixed.clear();
        self.fixed_ends.clear();
        self.fixed_polygon_ends.clear();
        for rings in polygons {
            self.read_polygon(rings)?;
        }
        Ok(())
    }

    /// Reads `rings` into fixed point without repeated vertices, dropping rings with fewer than three,
    /// and all of them when that is the exterior or there is none, and turning the exterior positive and
    /// the holes negative.
    fn read_polygon<R>(&mut self, rings: R) -> Result<(), TileError>
    where
        R: IntoIterator,
        R::Item: IntoIterator<Item = Coord<f64>>,
    {
        let first_ring = self.fixed_ends.len();
        for (i, ring) in rings.into_iter().enumerate() {
            let start = self.fixed.len();
            for c in ring {
                let c = Coord {
                    x: to_fixed(c.x)?,
                    y: to_fixed(c.y)?,
                };
                push_new(&mut self.fixed, start, c);
            }
            drop_closing(&mut self.fixed, start);
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
        if self.fixed_ends.len() > first_ring {
            self.fixed_polygon_ends.push(self.fixed_ends.len());
        }
        Ok(())
    }

    /// Whether the rounded polygons are valid.
    fn output_is_valid(&self) -> bool {
        let vertices: Vec<Fixed> = self.vertices.iter().map(|&p| widen(p)).collect();
        is_valid(&vertices, &self.ring_ends, &self.polygon_ends)
    }

    /// Rounds every vertex on its own, dropping the rings left with fewer than three, and a polygon's
    /// rings when that is its exterior.
    fn round(&mut self) {
        self.vertices.clear();
        self.ring_ends.clear();
        self.polygon_ends.clear();
        self.rounded_at.clear();
        for rings in spans(&self.fixed_polygon_ends) {
            let first_ring = self.ring_ends.len();
            for r in rings.clone() {
                let start = self.vertices.len();
                let from = self.rounded_at.len();
                for &c in &self.fixed[span(&self.fixed_ends, r)] {
                    push_new(&mut self.vertices, start, pixel(c));
                    self.rounded_at.push(Some(self.vertices.len() - 1));
                }
                if drop_closing(&mut self.vertices, start) {
                    let last = Some(self.vertices.len());
                    for at in &mut self.rounded_at[from..] {
                        if *at == last {
                            *at = Some(start);
                        }
                    }
                }
                if self.vertices.len() - start < 3 {
                    self.vertices.truncate(start);
                    if r == rings.start {
                        self.rounded_at.truncate(from);
                        self.rounded_at.resize(self.fixed_ends[rings.end - 1], None);
                        break;
                    }
                    self.rounded_at[from..].fill(None);
                    continue;
                }
                self.ring_ends.push(self.vertices.len());
            }
            if self.ring_ends.len() > first_ring {
                self.polygon_ends.push(self.ring_ends.len());
            }
        }
    }

    /// Keeps the rounded rings as they are, each exterior turning positive and each hole negative.
    fn keep(&mut self) {
        for rings in spans(&self.polygon_ends) {
            for r in rings.clone() {
                let ring = &mut self.vertices[span(&self.ring_ends, r)];
                if (signed_area_2x(ring) > 0) != (r == rings.start) {
                    ring.reverse();
                }
            }
        }
    }

    /// Lists the rounded edges, and tells whether no ring folds back onto itself at a vertex.
    fn rounded_edges(&mut self) -> bool {
        self.segs.clear();
        for (ring, span) in spans(&self.ring_ends).enumerate() {
            let ring_start = span.start;
            let verts = &self.vertices[span];
            let ring_len = verts.len();
            for (k, &at) in verts.iter().enumerate() {
                let (prev, next) = (verts[(k + ring_len - 1) % ring_len], verts[(k + 1) % ring_len]);
                if folds(widen(prev), widen(at), widen(next)) {
                    return false;
                }
                self.segs.push(Seg {
                    a: at,
                    b: next,
                    ring,
                    ring_start,
                    ring_len,
                });
            }
        }
        true
    }

    /// Whether the polygon is valid in fixed point and stayed valid when rounded, checking only the
    /// vertices within a unit of another edge.
    fn rounding_kept_valid(&mut self) -> bool {
        if !self.rounded_edges() {
            return false;
        }
        self.near_other_ring.clear();
        self.near_other_ring.resize(self.fixed.len(), false);
        let Self {
            fixed,
            fixed_ends,
            rounded_at,
            near_other_ring,
            segs,
            grid,
            ..
        } = self;
        let segs = &*segs;
        let incident = |r: usize| {
            let s = segs[r];
            [
                if r == s.ring_start {
                    s.ring_start + s.ring_len - 1
                } else {
                    r - 1
                },
                r,
            ]
        };
        let apart = |x: usize, y: usize| {
            x == y || adjacent(segs, x, y) || contact(segs[x].line(), segs[y].line()) == Contact::Apart
        };
        let planar = near_vertices(fixed, fixed_ends, grid, |v, a, b| {
            let (Some(rv), Some(ra)) = (rounded_at[v], rounded_at[a]) else {
                return true;
            };
            if !edges_at_clear_of(fixed, fixed_ends, v, a, b) {
                return false;
            }
            if segs[rv].ring != segs[ra].ring {
                near_other_ring[v] = true;
            }
            let image = if rounded_at[b] == Some(ra) {
                incident(ra)
            } else {
                [ra, ra]
            };
            incident(rv).iter().all(|&x| image.iter().all(|&y| apart(x, y)))
        });
        planar
            && rings_of(rounded_at, fixed_ends)
                .zip(rings_of(near_other_ring, fixed_ends))
                .all(|(r, near)| r[0].is_none() || near.contains(&false))
    }

    /// Whether the rounded rings meet only where consecutive edges share a vertex, listing their edges
    /// in `lines`.
    fn rounded_rings_apart(&mut self) -> bool {
        if !self.rounded_edges() {
            return false;
        }
        let segs = &self.segs;
        self.lines.clear();
        self.lines.extend(segs.iter().map(|s| s.line()));
        let lines = &self.lines;
        near_pairs(lines, &mut self.grid, |i, j| {
            adjacent(segs, i, j) || contact(lines[i], lines[j]) == Contact::Apart
        })
    }

    /// Whether the rounded rings meet only where consecutive edges share a vertex, with every hole
    /// inside the exterior and outside the other holes.
    fn rounded_is_valid(&mut self) -> bool {
        if !self.rounded_rings_apart() {
            return false;
        }
        let (segs, lines) = (&self.segs, &self.lines);
        if self.ring_ends.len() == 1 {
            return true;
        }
        let edges: Vec<(Fixed, Fixed)> = lines.iter().map(|&(a, b)| left_first(a, b)).collect();
        let hole_starts = &self.ring_ends[..self.ring_ends.len() - 1];
        let queries: Vec<Fixed> = hole_starts.iter().map(|&s| widen(self.vertices[s]) * 2).collect();
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

    /// Snap-rounds the polygons and rebuilds the area where `inside` holds as valid polygons.
    fn repair(&mut self, inside: fn(Winding) -> bool) {
        self.vertices.clear();
        self.ring_ends.clear();
        self.polygon_ends.clear();
        let mut edges = Vec::new();
        for rings in spans(&self.fixed_polygon_ends) {
            for r in rings.clone() {
                let ring = &self.fixed[span(&self.fixed_ends, r)];
                let winding = if r == rings.start {
                    Winding::EXTERIOR
                } else {
                    Winding::HOLE
                };
                for (k, &a) in ring.iter().enumerate() {
                    let b = ring[(k + 1) % ring.len()];
                    edges.push(FixedEdge { a, b, winding });
                }
            }
        }
        let hot = hot_pixels(&edges);
        let net = net_edges(&edges, &hot);
        let rings = trace(boundary(&net, inside));
        self.assemble(rings);
    }

    /// Groups traced rings into polygons: each hole goes to the smallest exterior around it.
    fn assemble(&mut self, rings: Vec<Vec<Pixel>>) {
        let (shells, holes): (Vec<_>, Vec<_>) = rings.into_iter().partition(|r| signed_area_2x(r) > 0);
        let mut owner = vec![Some(0); holes.len()];
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
            owner.fill(None);
            for (hole, shell) in odd_runs(hits) {
                if owner[hole].is_none_or(|o| areas[shell] < areas[o]) {
                    owner[hole] = Some(shell);
                }
            }
        }
        for (s, shell) in shells.iter().enumerate() {
            self.vertices.extend_from_slice(shell);
            self.ring_ends.push(self.vertices.len());
            for (hole, _) in holes.iter().zip(&owner).filter(|&(_, &o)| o == Some(s)) {
                self.vertices.extend_from_slice(hole);
                self.ring_ends.push(self.vertices.len());
            }
            self.polygon_ends.push(self.ring_ends.len());
        }
    }
}

/// Appends `v` to the ring starting at `start` unless it repeats the ring's last vertex.
fn push_new<T: PartialEq>(verts: &mut Vec<T>, start: usize, v: T) {
    if verts.len() == start || verts.last() != Some(&v) {
        verts.push(v);
    }
}

/// Drops the last vertex of the ring starting at `start` if it repeats the first, telling whether it did.
fn drop_closing<T: PartialEq>(verts: &mut Vec<T>, start: usize) -> bool {
    let closes = verts.len() > start + 1 && verts.last() == verts.get(start);
    if closes {
        verts.pop();
    }
    closes
}

/// Whether both edges at vertex `v` meet the edge from vertex `a` to vertex `b` only as a valid
/// polygon's edges can.
fn edges_at_clear_of(verts: &[Fixed], ends: &[usize], v: usize, a: usize, b: usize) -> bool {
    let ring = span(ends, ends.partition_point(|&e| e <= v));
    let prev = if v == ring.start { ring.end - 1 } else { v - 1 };
    let next = if v + 1 == ring.end { ring.start } else { v + 1 };
    let same_ring = ring.contains(&a);
    [(prev, v), (v, next)]
        .into_iter()
        .all(|(x, y)| match contact((verts[x], verts[y]), (verts[a], verts[b])) {
            Contact::Apart => true,
            Contact::Point => !same_ring || x == b || y == a,
            Contact::Cross | Contact::Overlap => false,
        })
}

/// Whether two rounded edges follow each other around a ring.
fn adjacent(segs: &[Seg], i: usize, j: usize) -> bool {
    segs[i].ring == segs[j].ring && (i.abs_diff(j) == 1 || i.abs_diff(j) == segs[i].ring_len - 1)
}

impl<'a> Quantized<'a> {
    /// Each polygon's rings, exterior first, open: exteriors have a positive [`signed_area_2x`], holes a
    /// negative one.
    ///
    /// [`signed_area_2x`]: crate::signed_area_2x
    pub fn polygons(self) -> impl Iterator<Item = impl Iterator<Item = &'a [Pixel]> + 'a> + 'a {
        let q = self.quantizer;
        spans(&q.polygon_ends).map(move |rings| rings.map(move |r| &q.vertices[span(&q.ring_ends, r)]))
    }
}

/// The index range of the `i`th ring or polygon of those ending at `ends`.
fn span(ends: &[usize], i: usize) -> Range<usize> {
    let start = if i == 0 { 0 } else { ends[i - 1] };
    start..ends[i]
}

/// The index range of each ring or polygon of those ending at `ends`.
fn spans(ends: &[usize]) -> impl Iterator<Item = Range<usize>> + '_ {
    (0..ends.len()).map(|i| span(ends, i))
}

/// The rings of `verts` that end at `ends`.
fn rings_of<'a, T>(verts: &'a [T], ends: &'a [usize]) -> impl Iterator<Item = &'a [T]> {
    spans(ends).map(|span| &verts[span])
}

/// `v` in fixed point, if its pixel fits `i32`, never on a half `v` is not on.
#[expect(clippy::cast_possible_truncation, reason = "range-checked before the cast")]
#[expect(clippy::cast_precision_loss, reason = "ONE and HALF are small powers of two")]
fn to_fixed(v: f64) -> Result<i64, TileError> {
    /// Adding and taking away `1.5 · 2^52` rounds anything below `2^51` to a whole number, ties to even.
    const ROUND: f64 = 6_755_399_441_055_744.0;
    let scaled = v * ONE as f64;
    let mut f = (scaled + ROUND) - ROUND;
    #[expect(
        clippy::float_cmp,
        reason = "exact on purpose: only a rounded `f` may move off a half"
    )]
    let rounded_onto_a_half = f != scaled && f.rem_euclid(ONE as f64) == HALF as f64;
    if rounded_onto_a_half {
        f += (scaled - f).signum();
    }
    let lo = f64::from(i32::MIN) * ONE as f64 - HALF as f64;
    let hi = (f64::from(i32::MAX) + 1.0) * ONE as f64 - HALF as f64;
    if f >= lo && f < hi {
        Ok(f as i64)
    } else {
        Err(TileError::Overflow)
    }
}

/// The pixel holding a fixed-point coordinate: the nearest grid point, halves to the even one as `PostGIS`
/// rounds. Along each axis, pixel `c` holds `[c - ½, c + ½]` when `c` is even and `(c - ½, c + ½)` when odd.
#[expect(clippy::cast_possible_truncation, reason = "to_fixed keeps every pixel in i32")]
fn pixel(c: Fixed) -> Pixel {
    // A tie rounds down from an even unit and up from an odd one; branch-free, as a `match` mispredicts.
    let unit = |v: i64| ((v + HALF - 1 + ((v >> FRAC) & 1)) >> FRAC) as i32;
    Coord {
        x: unit(c.x),
        y: unit(c.y),
    }
}

/// `numerator / denominator` (`denominator > 0`) rounded to the nearest integer, halves to the even one.
fn round_ratio(numerator: i128, denominator: i128) -> i128 {
    let quotient = numerator.div_euclid(denominator);
    let remainder = numerator.rem_euclid(denominator);
    match (2 * remainder).cmp(&denominator) {
        Ordering::Less => quotient,
        Ordering::Greater => quotient + 1,
        Ordering::Equal => quotient + quotient.rem_euclid(2),
    }
}

/// `p` with `i64` coordinates.
fn widen(p: Pixel) -> Coord<i64> {
    Coord {
        x: i64::from(p.x),
        y: i64::from(p.y),
    }
}

/// The lowest and highest corners of the box around `points`.
fn bounds(mut points: impl Iterator<Item = Fixed>) -> Option<(Fixed, Fixed)> {
    let first = points.next()?;
    Some(points.fold((first, first), |(min, max), c| {
        (
            Coord {
                x: min.x.min(c.x),
                y: min.y.min(c.y),
            },
            Coord {
                x: max.x.max(c.x),
                y: max.y.max(c.y),
            },
        )
    }))
}

/// Reusable working memory of [`near_pairs`].
#[derive(Debug, Default, Clone)]
struct Grid {
    cells: Vec<(u64, usize)>,
    lengths: Vec<i64>,
    columns: Vec<usize>,
    points: Vec<(usize, usize)>,
}

/// Whether `test(i, j)` holds for every two of `lines` that pass through a common cell of a grid sized
/// to their typical length, stopping at the first that fails.
fn near_pairs(lines: &[(Coord<i64>, Coord<i64>)], grid: &mut Grid, mut test: impl FnMut(usize, usize) -> bool) -> bool {
    let Grid { cells, lengths, .. } = grid;
    cells.clear();
    lengths.clear();
    let Some((min, max)) = bounds(lines.iter().flat_map(|&(a, b)| [a, b])) else {
        return true;
    };
    lengths.extend(lines.iter().map(|&(a, b)| (b.x - a.x).abs().max((b.y - a.y).abs())));
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

/// Up to this many vertices, [`near_vertices`] checks every vertex against every edge.
/// Building the grid costs more than it saves for values below it.
const GRID_MIN_VERTICES: usize = 32;

/// Whether `test(v, a, b)` holds for every vertex `v` within a unit of an edge from vertex `a` to vertex
/// `b` of the rings ending at `ends`, stopping at the first that fails.
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
    let edges = || {
        spans(ends)
            .flat_map(|Range { start, end }| (start..end).map(move |k| (k, if k + 1 == end { start } else { k + 1 })))
    };
    if verts.len() <= GRID_MIN_VERTICES {
        return edges().all(|(a, b)| {
            let (p, q) = (verts[a], verts[b]);
            verts
                .iter()
                .enumerate()
                .all(|(v, &c)| v == a || v == b || !passes_near(p, q, c) || test(v, a, b))
        });
    }
    lengths.clear();
    columns.clear();
    points.clear();
    let Some((min, max)) = bounds(verts.iter().copied()) else {
        return true;
    };
    lengths.extend(edges().map(|(a, b)| {
        let d = verts[b] - verts[a];
        d.x.abs().max(d.y.abs())
    }));
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
                if v != a && v != b && passes_near(p, q, verts[v]) && !test(v, a, b) {
                    return false;
                }
            }
        }
    }
    true
}

/// Whether the edge `a`-`b` passes within a unit of `v` along both axes.
fn passes_near(a: Fixed, b: Fixed, v: Fixed) -> bool {
    if a.x.min(b.x) > v.x + ONE || a.x.max(b.x) < v.x - ONE || a.y.min(b.y) > v.y + ONE || a.y.max(b.y) < v.y - ONE {
        return false;
    }
    let d = b - a;
    cross(d, v - a).abs() <= i128::from(ONE) * i128::from(d.x.abs() + d.y.abs())
}

/// How two edges meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Contact {
    /// Not at all.
    Apart,
    /// At one point only, an end of one of them.
    Point,
    /// Crossing inside both.
    Cross,
    /// Along a stretch.
    Overlap,
}

/// How the edges `p1`-`p2` and `q1`-`q2` meet.
fn contact((p1, p2): (Fixed, Fixed), (q1, q2): (Fixed, Fixed)) -> Contact {
    let boxed = |c: Fixed, a: Fixed, b: Fixed| {
        c.x >= a.x.min(b.x) && c.x <= a.x.max(b.x) && c.y >= a.y.min(b.y) && c.y <= a.y.max(b.y)
    };
    let side = |a: Fixed, b: Fixed, c: Fixed| cross(b - a, c - a).signum();
    let (s1, s2) = (side(p1, p2, q1), side(p1, p2, q2));
    let (s3, s4) = (side(q1, q2, p1), side(q1, q2, p2));
    if s1 * s2 < 0 && s3 * s4 < 0 {
        return Contact::Cross;
    }
    if s1 == 0 && s2 == 0 {
        let key = |c: Fixed| if p1.x == p2.x { c.y } else { c.x };
        let lo = key(p1).min(key(p2)).max(key(q1).min(key(q2)));
        let hi = key(p1).max(key(p2)).min(key(q1).max(key(q2)));
        return match lo.cmp(&hi) {
            Ordering::Less => Contact::Overlap,
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
            if folds(prev, at, next) {
                return false;
            }
            lines.push((at, next));
            ring_of.push(r);
        }
    }
    let ring_len = |r: usize| span(ring_ends, r).len();
    let simple = near_pairs(&lines, &mut Grid::default(), |i, j| match contact(lines[i], lines[j]) {
        Contact::Apart => true,
        Contact::Point => ring_of[i] != ring_of[j] || i.abs_diff(j) == 1 || i.abs_diff(j) == ring_len(ring_of[i]) - 1,
        Contact::Cross | Contact::Overlap => false,
    });
    if !simple {
        return false;
    }
    let mut polygon_of = Vec::new();
    let mut shell_of = Vec::new();
    for (p, rings) in spans(polygon_ends).enumerate() {
        polygon_of.extend(core::iter::repeat_n(p, rings.len()));
        shell_of.extend(core::iter::repeat_n(rings.start, rings.len()));
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

/// Whether a ring turns straight back at `at`, coming from `prev` and going on to `next`.
fn folds(prev: Fixed, at: Fixed, next: Fixed) -> bool {
    let (back, ahead) = (prev - at, next - at);
    cross(back, ahead) == 0 && dot(back, ahead) > 0
}

/// The edge from `a` to `b` with its lower `(x, y)` end first.
fn left_first(a: Fixed, b: Fixed) -> (Fixed, Fixed) {
    if (a.x, a.y) <= (b.x, b.y) { (a, b) } else { (b, a) }
}

/// The cross product of `a` and `b`.
fn cross(a: Fixed, b: Fixed) -> i128 {
    i128::from(a.x) * i128::from(b.y) - i128::from(a.y) * i128::from(b.x)
}

/// The dot product of `a` and `b`.
fn dot(a: Fixed, b: Fixed) -> i128 {
    i128::from(a.x) * i128::from(b.x) + i128::from(a.y) * i128::from(b.y)
}

/// Twice the signed area of a fixed-point ring.
fn fixed_area_2x(ring: &[Fixed]) -> i128 {
    let o = ring[0];
    ring.windows(2).map(|w| cross(w[0] - o, w[1] - o)).sum()
}

/// Every pixel holding a vertex or a point where two edges cross, sorted.
fn hot_pixels(edges: &[FixedEdge]) -> Vec<Pixel> {
    let mut hot: Vec<Pixel> = edges.iter().map(|e| pixel(e.a)).collect();
    let lines: Vec<(Fixed, Fixed)> = edges.iter().map(|e| (e.a, e.b)).collect();
    near_pairs(&lines, &mut Grid::default(), |i, j| {
        if contact(lines[i], lines[j]) == Contact::Cross {
            let ((p1, p2), (q1, q2)) = (lines[i], lines[j]);
            hot.push(crossing(p1, p2 - p1, q1, q2 - q1));
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
    let at = |p: i64, d: i64| {
        let numerator = i128::from(p) * den + num * i128::from(d);
        let denominator = den * i128::from(ONE);
        round_ratio(numerator, denominator) as i32
    };
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
    /// Orders two bounds by their parameter.
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
        // Halves round to the even pixel, so an odd one leaves out both ends of its span.
        let open = c % 2 != 0;
        let inside = if open {
            min < p0 && p0 < max
        } else {
            min <= p0 && p0 <= max
        };
        let (from, to) = match d.cmp(&0) {
            Ordering::Equal if inside => continue,
            Ordering::Equal => return false,

            Ordering::Greater => (
                Bound {
                    n: (min - p0).into(),
                    d: d.into(),
                    open,
                },
                Bound {
                    n: (max - p0).into(),
                    d: d.into(),
                    open,
                },
            ),
            Ordering::Less => (
                Bound {
                    n: (p0 - max).into(),
                    d: (-d).into(),
                    open,
                },
                Bound {
                    n: (p0 - min).into(),
                    d: (-d).into(),
                    open,
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
fn net_edges(edges: &[FixedEdge], hot: &[Pixel]) -> Vec<NetEdge> {
    let mut frags = Vec::new();
    let mut path: Vec<(i128, Pixel)> = Vec::new();
    for &FixedEdge { a: p, b: q, winding } in edges {
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
                path.push((dot(widen(h) * ONE - p, q - p), h));
            }
        }
        path.sort_unstable_by_key(|s| s.0);
        path.push((i128::MAX, pq));
        for w in path.windows(2) {
            let (a, b) = (w[0].1, w[1].1);
            frags.push(if (a.x, a.y) < (b.x, b.y) {
                NetEdge { a, b, winding }
            } else {
                NetEdge {
                    a: b,
                    b: a,
                    winding: -winding,
                }
            });
        }
    }
    frags.sort_unstable_by_key(|f| (f.a.x, f.a.y, f.b.x, f.b.y));
    let mut net: Vec<NetEdge> = Vec::new();
    for f in frags {
        match net.last_mut() {
            Some(n) if n.a == f.a && n.b == f.b => n.winding += f.winding,
            _ => net.push(f),
        }
    }
    net.retain(|n| n.winding != Winding::default());
    net
}

/// Calls `hit(query, edge)` for each edge, given left end first, that the upward ray from just right of
/// each query point, in doubled coordinates, crosses.
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

/// The edges between the area where `inside` holds and the rest, each directed with the area on its left.
fn boundary(net: &[NetEdge], inside: fn(Winding) -> bool) -> Vec<(Pixel, Pixel)> {
    let lines: Vec<(Fixed, Fixed)> = net.iter().map(|n| (widen(n.a), widen(n.b))).collect();
    let queries: Vec<Fixed> = lines.iter().map(|&(a, b)| a + b).collect();
    let mut above = vec![Winding::default(); net.len()];
    rays_up(&lines, &queries, |q, e| {
        if q != e {
            above[q] -= net[e].winding;
        }
    });
    let mut edges = Vec::new();
    for (n, w) in net.iter().zip(above) {
        let (left, right) = if n.a.x == n.b.x {
            (w + n.winding, w)
        } else {
            (w, w - n.winding)
        };
        match (inside(left), inside(right)) {
            (true, false) => edges.push((n.a, n.b)),
            (false, true) => edges.push((n.b, n.a)),
            _ => {}
        }
    }
    edges
}

/// Which half of the plane a direction points into, counter-clockwise from `+x`.
fn half_plane(d: Coord<i64>) -> u8 {
    u8::from(!(d.y > 0 || (d.y == 0 && d.x > 0)))
}

/// Orders directions counter-clockwise from `+x`.
fn by_angle(a: Coord<i64>, b: Coord<i64>) -> Ordering {
    half_plane(a).cmp(&half_plane(b)).then_with(|| 0.cmp(&cross(a, b)))
}

/// Walks the boundary `edges` into simple rings: at each vertex it turns as far left as it can, and a
/// walk that comes back to a vertex splits there.
fn trace(mut edges: Vec<(Pixel, Pixel)>) -> Vec<Vec<Pixel>> {
    edges.sort_unstable_by(|p, q| {
        (p.0.x, p.0.y)
            .cmp(&(q.0.x, q.0.y))
            .then_with(|| by_angle(widen(p.1) - widen(p.0), widen(q.1) - widen(q.0)))
    });
    let mut used = vec![false; edges.len()];
    let mut rings = Vec::new();
    let mut walk = Vec::new();
    for start in 0..edges.len() {
        if used[start] {
            continue;
        }
        walk.clear();
        let mut h = start;
        let closed = loop {
            used[h] = true;
            let (a, v) = edges[h];
            walk.push(a);
            let from = edges.partition_point(|e| (e.0.x, e.0.y) < (v.x, v.y));
            let to = edges.partition_point(|e| (e.0.x, e.0.y) <= (v.x, v.y));
            if from == to {
                break false;
            }
            let back = widen(a) - widen(v);
            let past =
                from + edges[from..to].partition_point(|e| by_angle(widen(e.1) - widen(v), back) == Ordering::Less);
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
