//! The public slicing API: [`Slicer`] takes **one geometry** at a time - a line, a point or a polygon -
//! and returns a lazy iterator over its slices: [`Slicer::line`] / [`Slicer::point`] for every tile
//! the geometry reaches, or [`Slicer::tile_line`] / [`Slicer::tile_point`] for one given tile.
//!
//! A multi-geometry is sliced one element at a time, by the caller: whoever keeps a feature's
//! attributes with each of its slices, tile by tile, has to do that for a multi-feature too, so the
//! slicer has no parts of its own to index. Slices of several elements that land in one tile are
//! simply several slices there, which the caller groups (a `MultiLineString` in that tile, say).
//!
//! A line run is never copied: it borrows the caller's vertices ([`Local`]) and is computed only when
//! the iterator reaches it, so a line with millions of vertices costs no memory beyond the iterator's
//! own few fields, and stopping early stops the work. A polygon piece is built in the slicer's
//! working memory, one at a time (a polygon inside one tile is lent as it is).
//!
//! Each call reads the whole geometry once, to bound it. That settles every possible overflow up
//! front: the call returns a [`TileError`] or nothing at all, and the line and point iterators are
//! plain [`Iterator`]s whose items cannot fail. (The polygon iterator still yields `Result`s, for a
//! piece too large to index; see [`PolygonSlices`].)
//!
//! **Order is unspecified** for the all-tiles iterators: a line that leaves and re-enters a tile
//! gives that tile several runs, each its own item.
//!
//! Runs follow the keep-original rule (see [`clip_polyline`](crate::clip_polyline)): a vertex is kept
//! when one of its segments touches the tile's buffered box, and a run breaks only where a vertex is
//! dropped. So a run is always a **contiguous range** of the line's vertices, which [`Local`] lends
//! together with the tile origin.
//!
//! In terms of a line's segments (skipping zero-length ones), with "touches" meaning
//! [`segment_intersects`] against the tile's buffered box, a run of tile `t` is the vertex range of
//! segments `k..=e` where
//! - `k` touches `t`, but neither `k - 1` nor `k - 2` does ([`Prev::run_started`]: the run starts
//!   there, and only there, so each run is yielded once, at its first segment), and
//! - the run ends at the first `e` after which neither `e + 1` nor `e + 2` touches `t`
//!   ([`extend_run`]: one missed segment is bridged, keeping both its endpoints).
//!
//! [`LineSlices`] finds each `k` while walking the segments in order, [`TileLines`] by scanning the
//! line for one tile, and both extend with the same [`extend_run`], so they agree by construction.
//!
//! In [`ClipMode::Unclipped`] the same walk and scan decide which tiles a geometry reaches, but each
//! such tile gets the whole geometry, once: no run is extended and no polygon piece is built.

use core::iter::FusedIterator;
use std::collections::BTreeSet;

use geo_types::Coord;

use crate::TileError;
use crate::clip_polyline::segment_intersects;
use crate::grid::{Grid, Located, SegmentTiles};
use crate::polygon::{
    FillRun, PolygonPiece, PolygonSlices, Scratch, clip_polygon_to_tile, fill_ring,
    whole_polygon_in_tile,
};
use crate::tile::TileId;
use crate::vertex::{PolyVertex, Vertex};

/// How a [`Slicer`] cuts lines and polygons into the tiles they reach (points are never cut, so they
/// ignore it).
///
/// Every mode slices the same inputs to the same tiles, with one difference in failures: a clipped
/// slicer can still fail building a piece too large to index (`GeometryTooLarge`) where an
/// unclipped one, which builds nothing, succeeds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ClipMode {
    /// Each tile gets only the part of the geometry that touches its buffered box: a line's runs, a
    /// polygon's piece (with synthetic corners), or a [`FillRun`] for tiles it covers entirely. A
    /// piece of a valid polygon is itself a valid polygon (OGC), beyond the buffered box too, so it
    /// can be used on its own; best effort, see the crate docs.
    #[default]
    Clipped,
    /// Each tile the geometry reaches - exactly the tiles [`Clipped`](Self::Clipped) gives anything -
    /// gets the **whole** geometry, once, in that tile's local frame: a line as a single
    /// [`LineSlice`] of all its vertices, a polygon as a [`PolygonPiece`] of all its rings (also in
    /// every tile it covers entirely: no [`FillRun`] is yielded). For renderers that clip themselves.
    Unclipped,
}

/// Slices one geometry at a time into per-tile pieces on an integer grid, lazily.
///
/// Coordinates are integers in tile space: a vertex belongs to tile `x.div_euclid(extent)`, whose own
/// area is `[0, extent)` in its local frame (`x - tile·extent`); every tile's clip box grows `buffer`
/// on each side. Slices come in that local frame, so a vertex outside the tile's own area - in its
/// buffer, a polygon's synthetic corner one unit past the buffer, or a line's first vertex beyond the
/// box - lies outside `[0, extent)`. Project, simplify, and scale float source data into this space
/// before slicing.
///
/// Generic over the [`Vertex`] type `V` (defaults to [`Coord<i32>`]): plain coordinates or
/// payload-carrying vertices (e.g. a per-vertex M value via [`Measured`](crate::Measured)), whose
/// payload rides through slicing unchanged.
///
/// The slicer never panics: bad input yields a [`TileError`] from the call itself (and, for a polygon
/// piece too large to index, as an `Err` item that ends the iteration).
///
/// Lines and polygons are clipped to each tile unless the slicer is built
/// [`with_clip_mode`](Self::with_clip_mode)`(`[`ClipMode::Unclipped`]`)`, which gives each tile the
/// whole geometry instead.
///
/// Lines and points take `&self`, so one slicer can serve many threads. Polygons take `&mut self`:
/// they are sliced in the slicer's reusable working memory, so use one slicer per worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slicer<V: Vertex = Coord<i32>> {
    grid: Grid,
    mode: ClipMode,
    /// Working memory for polygon slicing, allocated by the first polygon and kept for its capacity
    /// (never part of the slicer's value).
    poly: Scratch<V>,
}

impl<V: Vertex> Slicer<V> {
    /// Create a slicer with the given tile side / per-tile output resolution `extent` and `buffer`.
    ///
    /// # Errors
    ///
    /// - [`TileError::InvalidExtent`] if `extent` is `0`, or `extent + buffer` exceeds `i32::MAX`.
    /// - [`TileError::BufferTooLarge`] if `buffer` is not strictly less than half the `extent`.
    pub fn new(extent: u32, buffer: u16) -> Result<Self, TileError> {
        Ok(Self {
            grid: Grid::new(extent, buffer)?,
            mode: ClipMode::Clipped,
            poly: Scratch::new(),
        })
    }

    /// This slicer, cutting lines and polygons by `mode` ([`ClipMode::Clipped`] by default).
    #[must_use]
    pub fn with_clip_mode(mut self, mode: ClipMode) -> Self {
        self.mode = mode;
        self
    }

    /// How this slicer cuts lines and polygons.
    #[must_use]
    pub fn clip_mode(&self) -> ClipMode {
        self.mode
    }

    /// The tile side / per-tile output resolution: a tile's own area is `0..extent` in its local
    /// frame (its buffer and kept vertices beyond it lie outside).
    #[must_use]
    pub fn extent(&self) -> u32 {
        self.grid.extent()
    }

    /// The buffer kept around every tile, in tile-space units.
    #[must_use]
    pub fn buffer(&self) -> u16 {
        self.grid.buffer()
    }

    /// Release the working memory the slicer keeps between features. It only ever grows to what the
    /// largest polygon row or piece needed, but after an outlier a long-lived slicer would otherwise
    /// hold that peak forever.
    pub fn shrink_to_fit(&mut self) {
        self.poly = Scratch::new();
    }

    /// Slice a line into every tile it touches.
    ///
    /// Yields one [`LineSlice`] per run, in no particular order. A line that leaves a tile and comes
    /// back gives that tile several items. Consecutive duplicate vertices are skipped. For a
    /// multi-line, call this for each of its lines.
    ///
    /// [`ClipMode::Unclipped`] yields one item per tile instead, its run the whole line; telling a
    /// tile's later runs apart costs a set of the tiles yielded so far (memory proportional to
    /// their number, and several times the instructions of a clipped walk over a long diagonal
    /// line, where nearly every run starts a new tile).
    ///
    /// The call reads every vertex once to bound the line, so that nothing can fail once the
    /// iteration starts; the slices themselves are still computed one at a time as it advances.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if the line lies too close to the `i32` limits for this `extent` and
    /// `buffer`: a tile box beyond `i32`, or a vertex more than an `i32` span from the origin of a
    /// tile its bounding box reaches. (So a line spanning more than `2^31` units on one axis is
    /// rejected whole, even if each run would fit its tile.)
    pub fn line<'a>(&self, line: &'a [V]) -> Result<LineSlices<'a, V>, TileError> {
        if let Some((lo, hi)) = bounds(line) {
            self.grid.check_bounds(lo, hi)?;
        }
        Ok(LineSlices {
            grid: self.grid,
            pts: line,
            cursor: None,
            seen: (self.mode == ClipMode::Unclipped).then(BTreeSet::new),
        })
    }

    /// Clip a line to one `tile`: yields its runs there, each in walk order - exactly the runs
    /// [`line`](Self::line) yields for that tile. [`ClipMode::Unclipped`] yields the whole line
    /// once if it reaches the tile, stopping the scan there.
    ///
    /// Like [`line`](Self::line), the call reads every vertex once; a line whose bounds miss the
    /// tile's buffered box yields nothing without being scanned.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if `tile`'s buffered box overflows `i32` (a tile far outside the
    /// representable range for this `extent`), or if the line reaches the tile but has a vertex more
    /// than an `i32` span from the tile origin.
    pub fn tile_line<'a>(
        &self,
        tile: TileId,
        line: &'a [V],
    ) -> Result<TileLines<'a, V>, TileError> {
        let (min, max) = self.grid.tile_buffered_bounds(tile)?;
        let origin = self.grid.origin_of(min);
        let mut next = 0;
        match bounds(line) {
            Some((lo, hi)) if hi.x >= min.x && lo.x <= max.x && hi.y >= min.y && lo.y <= max.y => {
                // The line reaches the tile: every vertex must fit the local frame.
                let fits = |p: Coord<i32>| {
                    p.x.checked_sub(origin.x).is_some() && p.y.checked_sub(origin.y).is_some()
                };
                if !fits(lo) || !fits(hi) {
                    return Err(TileError::Overflow);
                }
            }
            // Nothing to scan: no vertices, or none near the tile.
            _ => next = line.len(),
        }
        Ok(TileLines {
            min,
            max,
            origin,
            tile,
            pts: line,
            next,
            whole: self.mode == ClipMode::Unclipped,
        })
    }

    /// Slice a point into every tile whose buffered box holds it: its own tile, plus up to three
    /// neighbors when it lies within `buffer` of an edge. Yields one [`PointSlice`] per tile. For a
    /// multi-point, call this for each of its points. The [`ClipMode`] does not apply.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if one of those tiles has a buffered box beyond `i32`.
    pub fn point(&self, point: V) -> Result<PointSlices<V>, TileError> {
        let p = point.position();
        self.grid.check_tiles(p, p)?;
        Ok(PointSlices::new(self.grid, point))
    }

    /// The `point` in `tile`'s local frame, if the tile's buffered box holds it.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if `tile`'s buffered box overflows `i32`.
    pub fn tile_point(&self, tile: TileId, point: V) -> Result<Option<V>, TileError> {
        let (extent, buffer) = (i64::from(self.grid.extent()), i64::from(self.grid.buffer()));
        let p = point.position();
        // Per axis: the tile's buffered box `[base - buffer, base + extent - 1 + buffer]`, which must fit
        // `i32` (what `Grid::tile_buffered_bounds` checks), and the point's offset from its base.
        let axis = |tile: i32, v: i32| {
            let base = i64::from(tile) * extent;
            let (lo, hi) = (base - buffer, base + extent - 1 + buffer);
            if lo < i64::from(i32::MIN) || hi > i64::from(i32::MAX) {
                return Err(TileError::Overflow);
            }
            let v = i64::from(v);
            // Inside the box, so within `extent + buffer` of the base: the offset fits `i32`.
            #[expect(clippy::cast_possible_truncation, reason = "within the box, see above")]
            Ok((lo..=hi).contains(&v).then_some((v - base) as i32))
        };
        let (x, y) = (axis(tile.x, p.x)?, axis(tile.y, p.y)?);
        Ok(x.zip(y).map(|(x, y)| point.with_position(Coord { x, y })))
    }
}

impl<V: PolyVertex> Slicer<V> {
    /// Slice a polygon into every tile it reaches. `rings` are its rings - the exterior first, then
    /// the holes - each open or closed (a repeated first/last vertex is fine). Rings with fewer than
    /// 3 vertices once consecutive duplicates are dropped are ignored, and so is a polygon whose
    /// exterior is. For a multipolygon, call this for each of its polygons.
    ///
    /// Yields a [`PolygonPiece`] for every tile a ring edge touches (its
    /// **edge tiles**), exactly what [`tile_polygon`](Self::tile_polygon) yields there, and a
    /// [`FillRun`] for every row span of tiles the polygon covers entirely. Output
    /// rings keep the winding of the input rings (see [`signed_area_2x`](crate::signed_area_2x) to
    /// normalize it first).
    ///
    /// [`ClipMode::Unclipped`] yields the same tiles, but each - those of the fill runs too, one by
    /// one - as a piece of the whole polygon: every kept ring, lent from the input. No piece is built.
    ///
    /// Lazy and lending: see [`PolygonSlices`]. The call reads the rings once, to bound and index
    /// them (only a ring with consecutive duplicate vertices is copied, to drop them); after that the
    /// iterator holds one tile row of the polygon at a time, and builds each piece as it is reached.
    /// Every row of the polygon's bounding box is visited, including rows where it yields nothing;
    /// [`PolygonSlices::within`] limits the slices, and the rows visited, to a window of tiles.
    ///
    /// # Errors
    ///
    /// - [`TileError::Overflow`] if the polygon lies too close to the `i32` limits for this `extent`
    ///   and `buffer`: a tile box (grown one unit, for the synthetic corners) beyond `i32`, or a
    ///   vertex more than an `i32` span from the origin of a tile its bounding box reaches.
    /// - [`TileError::GeometryTooLarge`] if there are more than `u32::MAX` rings or edges.
    ///
    /// The only error left for the iterator is a piece of more than `u32::MAX` vertices.
    #[inline]
    pub fn polygon<'s, R: AsRef<[V]>>(
        &'s mut self,
        rings: &'s [R],
    ) -> Result<PolygonSlices<'s, V, R>, TileError> {
        PolygonSlices::new(self.grid, self.mode, rings, self.poly.get())
    }

    /// Clip a polygon to one `tile`: its piece there, or `None` if it leaves nothing - what
    /// [`polygon`](Self::polygon) yields for that tile, with a covered tile's
    /// [`FillRun`] as its fill ring. The piece borrows the slicer's working memory
    /// until the next call. [`ClipMode::Unclipped`] gives the whole polygon instead, if it leaves
    /// anything in the tile, as [`polygon`](Self::polygon) does.
    ///
    /// # Errors
    ///
    /// - [`TileError::Overflow`] if `tile`'s buffered box overflows `i32`, or a synthetic corner or
    ///   kept vertex does.
    /// - [`TileError::GeometryTooLarge`] if there are more than `u32::MAX` rings, or the piece has
    ///   more than `u32::MAX` vertices.
    #[inline]
    pub fn tile_polygon<'s, R: AsRef<[V]>>(
        &'s mut self,
        tile: TileId,
        rings: &'s [R],
    ) -> Result<Option<PolygonPiece<'s, V, R>>, TileError> {
        u32::try_from(rings.len()).map_err(|_too_large| TileError::GeometryTooLarge)?;
        let s = &mut self.poly.get().scratch;
        match self.mode {
            ClipMode::Clipped => clip_polygon_to_tile(self.grid, tile, rings, s),
            ClipMode::Unclipped => whole_polygon_in_tile(self.grid, tile, rings, s),
        }
    }

    /// The ring [`tile_polygon`](Self::tile_polygon) yields for `run`'s polygon in each tile of the
    /// run: the tile's buffered box grown by one unit, tile-local and closed (its first vertex
    /// repeated), all synthetic vertices, wound like the polygon's exterior. See also
    /// [`PolygonSlices::fill_ring`], for use while the iterator borrows the slicer.
    #[must_use]
    pub fn fill_ring(&self, run: &FillRun) -> [V; 5] {
        fill_ring(self.grid, run)
    }
}

/// Vertices in a tile's local frame - `v - origin`, computed as they are read - every position checked
/// to fit `i32` before the slice is handed out.
///
/// A line run is lent from the caller's input, and skips consecutive duplicate positions (the first
/// of each is kept). A polygon ring has no consecutive duplicates, and is closed: its first vertex
/// repeated at the end, which a ring lent open has [`iter`](Self::iter) append (drop it for an MVT
/// `ClosePath`). It is lent from the input when the polygon lies inside one tile, when it is one small
/// ring inside the tile's buffered box, or when the slicer is [`ClipMode::Unclipped`]; else built in
/// the slicer's working memory, clipped and with synthetic corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Local<'a, V> {
    verts: &'a [V],
    origin: Coord<i32>,
    shape: Shape,
}

/// What [`Local::iter`] does besides subtracting the origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A line run: skip consecutive duplicates.
    Run,
    /// A distinct ring that is already closed.
    Ring,
    /// A distinct ring lent open: repeat its first vertex at the end.
    OpenRing,
}

impl<'a, V: Vertex> Local<'a, V> {
    /// A line run, lent as it is.
    pub(crate) fn run(verts: &'a [V], origin: Coord<i32>) -> Self {
        Self {
            verts,
            origin,
            shape: Shape::Run,
        }
    }

    /// A polygon ring (distinct vertices), closed by `iter` unless it `is_closed` already.
    pub(crate) fn ring(verts: &'a [V], origin: Coord<i32>, is_closed: bool) -> Self {
        Self {
            verts,
            origin,
            shape: if is_closed {
                Shape::Ring
            } else {
                Shape::OpenRing
            },
        }
    }

    /// The vertices in the tile's local frame.
    #[must_use]
    #[inline]
    pub fn iter(&self) -> LocalIter<'a, V> {
        LocalIter {
            verts: self.verts.iter(),
            origin: self.origin,
            dedupe: self.shape == Shape::Run,
            last: None,
            closing: if self.shape == Shape::OpenRing {
                self.verts.first()
            } else {
                None
            },
        }
    }

    /// The vertices collected into a `Vec`, in the tile's local frame.
    #[must_use]
    pub fn to_vec(self) -> Vec<V> {
        self.iter().collect()
    }

    /// The vertices in the **global** frame: each local vertex plus [`origin`](Self::origin),
    /// consecutive duplicates included. For a line run this is a subslice of the input line, so its
    /// position in the line tells which vertices the run covers; for a clipped polygon ring it is the
    /// slicer's built ring. For a ring lent open this is without the repeated first vertex.
    #[must_use]
    pub fn global(&self) -> &'a [V] {
        self.verts
    }

    /// The tile origin (`tile · extent`) the local frame is relative to.
    #[must_use]
    pub fn origin(&self) -> Coord<i32> {
        self.origin
    }
}

impl<'a, V: Vertex> IntoIterator for &Local<'a, V> {
    type Item = V;
    type IntoIter = LocalIter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, V: Vertex> IntoIterator for Local<'a, V> {
    type Item = V;
    type IntoIter = LocalIter<'a, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over a [`Local`]'s vertices.
#[derive(Debug, Clone)]
pub struct LocalIter<'a, V> {
    verts: core::slice::Iter<'a, V>,
    origin: Coord<i32>,
    /// Whether to skip consecutive duplicates, and the last position seen if so.
    dedupe: bool,
    last: Option<Coord<i32>>,
    /// The vertex that closes the ring, once the rest is done.
    closing: Option<&'a V>,
}

impl<V: Vertex> LocalIter<'_, V> {
    /// `v` in the local frame (checked to fit before the slice was yielded).
    #[inline]
    fn local(&self, v: V) -> V {
        let p = v.position();
        v.with_position(Coord {
            x: p.x.wrapping_sub(self.origin.x),
            y: p.y.wrapping_sub(self.origin.y),
        })
    }
}

impl<V: Vertex> Iterator for LocalIter<'_, V> {
    type Item = V;

    #[inline]
    fn next(&mut self) -> Option<V> {
        for &v in self.verts.by_ref() {
            if self.dedupe {
                let p = v.position();
                if self.last == Some(p) {
                    continue;
                }
                self.last = Some(p);
            }
            return Some(self.local(v));
        }
        let closing = *self.closing.take()?;
        Some(self.local(closing))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let closing = usize::from(self.closing.is_some());
        let left = self.verts.len();
        // Skipped duplicates make a run shorter, but never empty while vertices are left.
        let low = if self.dedupe {
            usize::from(left > 0)
        } else {
            left
        };
        (low + closing, Some(left + closing))
    }
}

impl<V: Vertex> FusedIterator for LocalIter<'_, V> {}

/// One run of a line in one tile: a contiguous piece of it, in the tile's local frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineSlice<'a, V> {
    tile: TileId,
    run: Local<'a, V>,
}

impl<'a, V: Vertex> LineSlice<'a, V> {
    /// The tile this run belongs to.
    #[must_use]
    pub fn tile(&self) -> TileId {
        self.tile
    }

    /// The run's vertices, in the tile's local frame (at least two distinct positions): the whole line
    /// when [`ClipMode::Unclipped`].
    #[must_use]
    pub fn run(&self) -> Local<'a, V> {
        self.run
    }
}

/// The index of the first vertex after `i` at a different position, if any.
fn next_distinct<V: Vertex>(pts: &[V], i: usize) -> Option<usize> {
    let p = pts.get(i)?.position();
    (i + 1..pts.len()).find(|&j| pts[j].position() != p)
}

/// The bounding box `(lo, hi)` of every vertex of `pts`, or `None` if there is none. One pass, which
/// is what lets the iterators skip every per-vertex overflow check afterwards.
fn bounds<V: Vertex>(pts: &[V]) -> Option<(Coord<i32>, Coord<i32>)> {
    let (mut lo, mut hi) = (
        Coord {
            x: i32::MAX,
            y: i32::MAX,
        },
        Coord {
            x: i32::MIN,
            y: i32::MIN,
        },
    );
    for v in pts {
        let p = v.position();
        (lo.x, lo.y) = (lo.x.min(p.x), lo.y.min(p.y));
        (hi.x, hi.y) = (hi.x.max(p.x), hi.y.max(p.y));
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

/// Extend a run that has reached vertex `end` (inside the line `pts`) while the next segment, or the one
/// after it (a single-segment excursion out and back), touches the box `[min, max]`. Returns the
/// run's last vertex index.
fn extend_run<V: Vertex>(pts: &[V], mut end: usize, min: Coord<i32>, max: Coord<i32>) -> usize {
    while let Some(n1) = next_distinct(pts, end) {
        let (a, b) = (pts[end].position(), pts[n1].position());
        if segment_intersects(a, b, min, max) {
            end = n1;
            continue;
        }
        let Some(n2) = next_distinct(pts, n1) else {
            break;
        };
        if segment_intersects(b, pts[n2].position(), min, max) {
            end = n2;
            continue;
        }
        break;
    }
    end
}

/// A segment, as its two endpoints.
type Segment = (Coord<i32>, Coord<i32>);

/// The two segments before the current one in the line, most recent first.
#[derive(Debug, Clone, Copy, Default)]
struct Prev([Option<Segment>; 2]);

impl Prev {
    /// `self` after `seg`, the segment just left, joins it.
    #[inline]
    fn pushed(self, seg: Segment) -> Self {
        Self([Some(seg), self.0[0]])
    }

    /// Whether the previous segment touches the box `[min, max]`: the current one only continues its
    /// run there.
    #[inline]
    fn continues(self, min: Coord<i32>, max: Coord<i32>) -> bool {
        self.0[0].is_some_and(|(a, b)| segment_intersects(a, b, min, max))
    }

    /// Whether a run in the box `[min, max]` already started before the current segment: the previous
    /// one continues it, and the one before bridges a single missed segment (see [`extend_run`]).
    #[expect(
        clippy::inline_always,
        reason = "run per tile of a walk; left out of line it costs the line benchmarks 3%"
    )]
    #[inline(always)]
    fn run_started(self, min: Coord<i32>, max: Coord<i32>) -> bool {
        self.0
            .iter()
            .flatten()
            .any(|&(a, b)| segment_intersects(a, b, min, max))
    }
}

/// The tiles of the current segment still to be considered.
#[derive(Debug, Clone)]
enum SegTiles {
    /// The whole segment lies in one tile's inner box, so it touches only that tile.
    One(TileId),
    /// Any other segment: walked row by row through exactly the tiles it touches.
    Walk(SegmentTiles),
    /// Nothing left: every tile is done, or the segment only continues a run already yielded.
    Done,
}

/// Position inside a [`LineSlices`] walk.
#[derive(Debug, Clone)]
struct LineCursor {
    /// The current segment, as indices of its two (distinct) vertices.
    a: usize,
    c: usize,
    prev: Prev,
    tiles: SegTiles,
    /// The tile owning `c`, carried to the next segment's fast-path test.
    loc_c: Located,
}

/// Where the next run starts: its tile and buffered box, and its first segment's two vertex indices.
struct RunAt {
    tile: TileId,
    min: Coord<i32>,
    max: Coord<i32>,
    first: usize,
    second: usize,
}

/// A segment inside one tile's inner box touches only that tile; when the previous segment touches it
/// too, the segment only continues a run already yielded. So do all the following segments that stay
/// in that inner box: jump past them at once, with one containment test each, and leave the cursor on
/// the last of them with nothing to yield ([`SegTiles::Done`]).
///
/// Only the previous segment is consulted, not the one before: a segment that merely bridges to a
/// run takes the general path in [`LineSlices::next_run`].
fn skip_inside<V: Vertex>(grid: Grid, pts: &[V], cur: &mut LineCursor) {
    let loc = cur.loc_c;
    let (min, max) = grid.box_of(&loc);
    if !cur.prev.continues(min, max) {
        return;
    }
    while let Some(n) = next_distinct(pts, cur.c) {
        if !loc.contains_inner(pts[n].position()) {
            break;
        }
        cur.prev = cur
            .prev
            .pushed((pts[cur.a].position(), pts[cur.c].position()));
        (cur.a, cur.c) = (cur.c, n);
    }
    cur.tiles = SegTiles::Done;
}

/// Lazy iterator over the runs of a line in every tile it touches; see [`Slicer::line`].
#[derive(Debug, Clone)]
pub struct LineSlices<'a, V> {
    grid: Grid,
    pts: &'a [V],
    cursor: Option<LineCursor>,
    /// [`ClipMode::Unclipped`]: the tiles already given the whole line.
    seen: Option<BTreeSet<TileId>>,
}

impl<V: Vertex> LineSlices<'_, V> {
    /// Route the segment `a`-`c`, with `la` the tile owning `a`.
    fn route(&self, a: usize, c: usize, la: Located) -> (SegTiles, Located) {
        let (pa, pc) = (self.pts[a].position(), self.pts[c].position());
        if la.contains_inner(pa) && la.contains_inner(pc) {
            return (SegTiles::One(la.owner), la);
        }
        let loc_c = if la.contains_core(pc) {
            la
        } else {
            self.grid.locate_fit(pc)
        };
        (SegTiles::Walk(SegmentTiles::new(self.grid, pa, pc)), loc_c)
    }

    /// Move to the next segment; `false` when there is none.
    fn advance(&mut self) -> bool {
        let pts = self.pts;
        // The next segment: its first vertex, second vertex, the segments before it, and the tile
        // owning the first vertex if the walk already knows it (everywhere but at the start).
        let next = match &self.cursor {
            None => next_distinct(pts, 0).map(|c| (0, c, Prev::default(), None)),
            Some(cur) => next_distinct(pts, cur.c).map(|c| {
                let seg = (pts[cur.a].position(), pts[cur.c].position());
                (cur.c, c, cur.prev.pushed(seg), Some(cur.loc_c))
            }),
        };
        let Some((a, c, prev, known)) = next else {
            return false;
        };
        let la = known.unwrap_or_else(|| self.grid.locate_fit(pts[a].position()));
        let (tiles, loc_c) = self.route(a, c, la);
        let mut cur = LineCursor {
            a,
            c,
            prev,
            tiles,
            loc_c,
        };
        if matches!(cur.tiles, SegTiles::One(_)) {
            skip_inside(self.grid, pts, &mut cur);
        }
        self.cursor = Some(cur);
        true
    }

    /// Where the next run lies, if there is one.
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn next_run(&mut self) -> Option<RunAt> {
        loop {
            let Some(cur) = &mut self.cursor else {
                if !self.advance() {
                    return None;
                }
                continue;
            };
            // A segment in one tile's inner box touches only that tile, whose box is cached.
            let (tile, fast) = match &mut cur.tiles {
                SegTiles::One(tile) => {
                    let tile = *tile;
                    cur.tiles = SegTiles::Done;
                    (Some(tile), true)
                }
                SegTiles::Walk(walk) => (walk.next(), false),
                SegTiles::Done => (None, false),
            };
            let Some(tile) = tile else {
                if !self.advance() {
                    return None;
                }
                continue;
            };
            let (min, max) = if fast {
                self.grid.box_of(&cur.loc_c)
            } else {
                self.grid.tile_box(tile)
            };
            if cur.prev.run_started(min, max) {
                continue;
            }
            return Some(RunAt {
                tile,
                min,
                max,
                first: cur.a,
                second: cur.c,
            });
        }
    }
}

impl<'a, V: Vertex> Iterator for LineSlices<'a, V> {
    type Item = LineSlice<'a, V>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let at = self.next_run()?;
            let run = match &mut self.seen {
                None => &self.pts[at.first..=extend_run(self.pts, at.second, at.min, at.max)],
                Some(seen) => {
                    if !seen.insert(at.tile) {
                        continue; // a later run of a tile that already has the whole line
                    }
                    self.pts
                }
            };
            return Some(LineSlice {
                tile: at.tile,
                run: Local::run(run, self.grid.origin_of(at.min)),
            });
        }
    }
}

impl<V: Vertex> FusedIterator for LineSlices<'_, V> {}

/// Lazy iterator over the runs of a line in one tile; see [`Slicer::tile_line`].
#[derive(Debug, Clone)]
pub struct TileLines<'a, V> {
    tile: TileId,
    min: Coord<i32>,
    max: Coord<i32>,
    origin: Coord<i32>,
    pts: &'a [V],
    /// The vertex the scan resumes from.
    next: usize,
    /// [`ClipMode::Unclipped`]: the first run found is the whole line, and the last.
    whole: bool,
}

impl<'a, V: Vertex> Iterator for TileLines<'a, V> {
    type Item = LineSlice<'a, V>;

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn next(&mut self) -> Option<Self::Item> {
        let (pts, min, max) = (self.pts, self.min, self.max);
        // Find the next segment touching the box; the run starts at its first vertex.
        let mut a = self.next;
        while a < pts.len() {
            let Some(c) = next_distinct(pts, a) else {
                break;
            };
            if segment_intersects(pts[a].position(), pts[c].position(), min, max) {
                let run = if self.whole {
                    self.next = pts.len();
                    pts
                } else {
                    let end = extend_run(pts, c, min, max);
                    self.next = end;
                    &pts[a..=end]
                };
                return Some(LineSlice {
                    tile: self.tile,
                    run: Local::run(run, self.origin),
                });
            }
            a = c;
        }
        self.next = pts.len();
        None
    }
}

impl<V: Vertex> FusedIterator for TileLines<'_, V> {}

/// One axis of a point's tiles: its own, and the neighbor within `buffer` of an edge (the buffer is
/// under half a tile, so at most one per axis). The point lies `rem` past its own tile's origin; the
/// tile at offset `d` from its own (`-1`, `0` or `1`) has the point `rem - d · extent` past its
/// origin.
#[derive(Debug, Clone, Copy)]
struct AxisTiles {
    /// The point's own tile, and its offset from that tile's origin.
    tile: i32,
    rem: i32,
    /// The first offset `d` (`-1` when the previous tile holds the point too, else `0`), and how many
    /// tiles there are.
    first: i32,
    len: u8,
}

impl AxisTiles {
    /// One division. A neighbor needs `buffer > 0`, so `extent >= 3` and the neighbor's index stays in
    /// `i32`; the offsets fit because [`Slicer::point`] proved the neighbor's whole buffered box does.
    fn new(v: i32, extent: i32, buffer: i32) -> Self {
        // Floor division with one `/` and `%` (`div_euclid` + `rem_euclid` would divide twice).
        let (mut tile, mut rem) = (v / extent, v % extent);
        if rem < 0 {
            (tile, rem) = (tile - 1, rem + extent);
        }
        // In the first `buffer` units of its tile the point is in the previous tile's buffer, in the
        // last `buffer` units in the next one's (with `buffer == 0` neither).
        let (before, after) = (rem < buffer, rem >= extent - buffer);
        Self {
            tile,
            rem,
            first: -i32::from(before),
            len: 1 + u8::from(before) + u8::from(after),
        }
    }
}

/// A point in one tile, in the tile's local frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointSlice<V> {
    tile: TileId,
    point: V,
}

impl<V: Vertex> PointSlice<V> {
    /// The tile this point lands in.
    #[must_use]
    #[inline]
    pub fn tile(&self) -> TileId {
        self.tile
    }

    /// The point in the tile's local frame: `position - tile · extent`, the payload unchanged.
    #[must_use]
    #[inline]
    pub fn point(&self) -> V {
        self.point
    }
}

/// The tiles a point lands in, with the point in each; see [`Slicer::point`]. One to four items, columns
/// first, computed with a single division per axis.
#[derive(Debug, Clone)]
pub struct PointSlices<V> {
    point: V,
    extent: i32,
    x: AxisTiles,
    y: AxisTiles,
    /// The next item, `0..x.len * y.len`.
    next: u8,
}

impl<V: Vertex> PointSlices<V> {
    fn new(grid: Grid, point: V) -> Self {
        let p = point.position();
        let (extent, buffer) = (grid.extent_i32(), grid.buffer_i32());
        Self {
            point,
            extent,
            x: AxisTiles::new(p.x, extent, buffer),
            y: AxisTiles::new(p.y, extent, buffer),
            next: 0,
        }
    }
}

impl<V: Vertex> Iterator for PointSlices<V> {
    type Item = PointSlice<V>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.x.len * self.y.len {
            return None;
        }
        // Columns first: the column is `next / y.len` and the row `next % y.len`, with `y.len` 1 or 2.
        let shift = self.y.len - 1;
        let (i, j) = (self.next >> shift, self.next & shift);
        self.next += 1;
        let (dx, dy) = (self.x.first + i32::from(i), self.y.first + i32::from(j));
        Some(PointSlice {
            tile: TileId::new(self.x.tile + dx, self.y.tile + dy),
            point: self.point.with_position(Coord {
                x: self.x.rem - dx * self.extent,
                y: self.y.rem - dy * self.extent,
            }),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = usize::from(self.x.len * self.y.len - self.next);
        (left, Some(left))
    }
}

impl<V: Vertex> ExactSizeIterator for PointSlices<V> {}

impl<V: Vertex> FusedIterator for PointSlices<V> {}
