//! The stateless slicing engine under the [`Slicer`](crate::Slicer).
//!
//! [`Grid`] holds only the tile geometry (extent + buffer): tile boxes and owners, the lazy
//! per-segment tile walk ([`SegmentTiles`]) behind the line slicer, and the per-row column span
//! ([`Seg::row_span`]) behind the polygon row scan. It keeps no accumulated state.

use geo_types::Coord;

use crate::TileError;
use crate::tile::{TileId, tile_of};
use crate::vertex::Vertex;

/// `c` shifted by `d` on both axes. Used for the `± buffer` corner offsets, where the caller has
/// already proved the result stays in `i32` (so no checked arithmetic).
#[inline]
const fn shift(c: Coord<i32>, d: i32) -> Coord<i32> {
    Coord {
        x: c.x + d,
        y: c.y + d,
    }
}

/// `⌊n·m / d⌋` for `d > 0`, exactly. The product of two coordinate spans can exceed `i64`, so it
/// falls back to `i128` - only for spans beyond ~3 billion units, so the common case stays in `i64`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "callers pass |n| <= d, so the quotient is within |m| and fits i64"
)]
fn mul_div_floor(n: i64, m: i64, d: i64) -> i64 {
    match n.checked_mul(m) {
        Some(p) => p.div_euclid(d),
        None => (i128::from(n) * i128::from(m)).div_euclid(i128::from(d)) as i64,
    }
}

/// A vertex's owner tile with its core cell and inner box precomputed in **global** coordinates, so
/// membership tests are plain comparisons with no division. The line slicer caches the last one
/// across the vertex walk: consecutive vertices in the same tile reuse it, and a segment whose two
/// endpoints both lie in the inner box touches only that one tile.
///
/// - the **core cell** `[base, base + extent - 1]` is the tile's own cell (owning `base = owner ·
///   extent`); a coordinate here has this tile as its owner.
/// - the **inner box** `[base + buffer, base + extent - 1 - buffer]` is the core shrunk by the
///   buffer; a segment with both endpoints inside it stays ≥ `buffer` from every edge, so it cannot
///   reach any neighboring tile's buffered box.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Located {
    pub(crate) owner: TileId,
    core_lo: Coord<i32>,
    core_hi: Coord<i32>,
    inner_lo: Coord<i32>,
    inner_hi: Coord<i32>,
}

impl Located {
    /// Does `c`'s owner tile equal this one (is `c` in the core cell)?
    pub(crate) fn contains_core(&self, c: Coord<i32>) -> bool {
        c.x >= self.core_lo.x
            && c.x <= self.core_hi.x
            && c.y >= self.core_lo.y
            && c.y <= self.core_hi.y
    }

    /// Is `c` in the inner box (≥ `buffer` from every cell edge)?
    pub(crate) fn contains_inner(&self, c: Coord<i32>) -> bool {
        c.x >= self.inner_lo.x
            && c.x <= self.inner_hi.x
            && c.y >= self.inner_lo.y
            && c.y <= self.inner_hi.y
    }
}

/// A segment ordered by `y` (`lo.y <= hi.y`), widened to `i64`, for the row-by-row walk.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Seg {
    lo: Coord<i64>,
    hi: Coord<i64>,
    dx: i64,
    dy: i64,
}

impl Seg {
    pub(crate) fn new(lo: Coord<i64>, hi: Coord<i64>) -> Self {
        Self {
            lo,
            hi,
            dx: hi.x - lo.x,
            dy: hi.y - lo.y,
        }
    }

    /// `x` where the segment is at offset `off` (`0..=dy`) above `lo.y`, rounded up or down.
    #[inline]
    fn x_at(self, off: i64, up: bool) -> i64 {
        match off {
            0 => self.lo.x,
            _ if off == self.dy => self.hi.x,
            _ if up => self.lo.x - mul_div_floor(-off, self.dx, self.dy),
            _ => self.lo.x + mul_div_floor(off, self.dx, self.dy),
        }
    }

    /// The sub-segment's integer `x`-extent inside row `ty`'s buffered slab: empty (`min > max`)
    /// when it runs between two integer columns there.
    #[inline]
    fn row_x(self, ty: i64, extent: i64, buffer: i64) -> (i64, i64) {
        // The row's buffered slab clipped to the segment's `y`-range, as offsets from `lo.y`; never
        // empty, since only rows whose slab meets `[lo.y, hi.y]` are asked for.
        let base = ty * extent;
        let start = (base - buffer).max(self.lo.y) - self.lo.y;
        let end = (base + extent - 1 + buffer).min(self.hi.y) - self.lo.y;
        // `x(y) = lo.x + (y - lo.y)·dx/dy` at the slab's two ends (monotone in `y`): rounding the low
        // end up and the high end down keeps exactly the integers it spans, and so the closed integer
        // boxes it meets. An end at a vertex is exact, which spares the division for most short
        // segments.
        if self.dy == 0 {
            (self.lo.x.min(self.hi.x), self.lo.x.max(self.hi.x))
        } else if self.dx >= 0 {
            (self.x_at(start, true), self.x_at(end, false))
        } else {
            (self.x_at(end, true), self.x_at(start, false))
        }
    }

    /// The tile columns `first..=last` of row `ty` whose buffered boxes the segment touches
    /// (`first > last` for a row it crosses between two unbuffered tiles).
    #[inline]
    pub(crate) fn row_span(self, ty: i64, extent: i64, buffer: i64) -> (i64, i64) {
        let (x_min, x_max) = self.row_x(ty, extent, buffer);
        (
            (x_min - buffer).div_euclid(extent),
            (x_max + buffer).div_euclid(extent),
        )
    }

    /// For a row `ty` the segment crosses without touching any tile (possible only with `buffer ==
    /// 0`, between two integer columns): the next row that can hold an integer `x`. The rows in
    /// between keep `x` strictly between the same two integers, so they touch nothing either.
    fn next_row_after_gap(self, ty: i64, extent: i64) -> i64 {
        if self.dx == 0 {
            return ty + 1; // a vertical segment sits on an integer column: never a gap
        }
        let (x_min, x_max) = self.row_x(ty, extent, 0);
        // The next integer the segment reaches as `y` grows, and the offset where it gets there:
        // the first integer `off` with `x(lo.y + off)` at or past it.
        let n = if self.dx > 0 {
            x_max + 1 - self.lo.x
        } else {
            self.lo.x - (x_min - 1)
        };
        // `|n| <= |dx|`, so the quotient stays within `dy`.
        let off = -mul_div_floor(-n, self.dy, self.dx.abs());
        ((self.lo.y + off).div_euclid(extent)).max(ty + 1)
    }
}

/// The tiles whose buffered box a segment touches, lazily: rows ascending, columns ascending within a
/// row, with the rows a segment crosses between two unbuffered tiles skipped at once instead of one
/// by one.
#[derive(Clone, Debug)]
pub(crate) struct SegmentTiles {
    seg: Seg,
    extent: i64,
    buffer: i64,
    /// The next row to look at, and the last row.
    ty: i64,
    ty_end: i64,
    /// The row being yielded, and its next and last columns.
    row: i64,
    tx: i64,
    tx_end: i64,
}

impl SegmentTiles {
    pub(crate) fn new(grid: Grid, a: Coord<i32>, c: Coord<i32>) -> Self {
        let (extent, buffer) = (i64::from(grid.extent), i64::from(grid.buffer));
        let wide = |p: Coord<i32>| Coord {
            x: i64::from(p.x),
            y: i64::from(p.y),
        };
        let (lo, hi) = if a.y <= c.y {
            (wide(a), wide(c))
        } else {
            (wide(c), wide(a))
        };
        Self {
            seg: Seg::new(lo, hi),
            extent,
            buffer,
            ty: (lo.y - buffer).div_euclid(extent),
            ty_end: (hi.y + buffer).div_euclid(extent),
            row: 0,
            tx: 1,
            tx_end: 0,
        }
    }
}

impl Iterator for SegmentTiles {
    /// A touched tile. The caller has proved (see [`Grid::check_tiles`]) that every tile the segment
    /// can touch has an `i32` id.
    type Item = TileId;

    #[expect(
        clippy::cast_possible_truncation,
        reason = "tile ids were proved to fit i32 by Grid::check_tiles"
    )]
    fn next(&mut self) -> Option<TileId> {
        loop {
            if self.tx <= self.tx_end {
                let tile = TileId::new(self.tx as i32, self.row as i32);
                self.tx += 1;
                return Some(tile);
            }
            if self.ty > self.ty_end {
                return None;
            }
            let ty = self.ty;
            let (first, last) = self.seg.row_span(ty, self.extent, self.buffer);
            if first <= last {
                (self.row, self.tx, self.tx_end) = (ty, first, last);
                self.ty = ty + 1;
            } else {
                self.ty = self.seg.next_row_after_gap(ty, self.extent);
            }
        }
    }
}

/// The tile geometry a slicer clips against: the tile side ([`extent`](Self::extent)) and a
/// [`buffer`](Self::buffer), plus the clipping engine.
///
/// Integers in pre-scaled tile space: `x` belongs to tile `x.div_euclid(extent)` and is emitted at
/// `x - tile·extent ∈ [0, extent)`, so `extent` is both the tile side and its output resolution; each
/// clip box grows `buffer` on every side. The library owns no float/projection math (callers scale
/// into this space up front), keeps original vertices, and never panics - bad input yields a
/// [`TileError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Grid {
    /// Tile side length in tile space, i.e. the per-tile output resolution (always in `1..=i32::MAX`).
    extent: i32,
    /// Margin, in tile-space units, kept around every tile (always in `0..=u16::MAX`).
    buffer: i32,
}

impl Grid {
    /// Create a grid with the given tile side / output resolution `extent` and `buffer`.
    ///
    /// # Errors
    ///
    /// - [`TileError::InvalidExtent`] if `extent` is `0`, or `extent + buffer` is greater than
    ///   `i32::MAX` - no tile's buffered box (grown one more unit for a polygon's fill box) would fit
    ///   `i32`, so nothing could be sliced.
    /// - [`TileError::BufferTooLarge`] if `2 * buffer >= extent` - the buffer must stay under half a
    ///   tile, so a vertex near an edge spills into at most one neighbor per axis and the
    ///   tile-minus-buffer inner box stays non-empty (both relied on by the routing).
    pub(crate) const fn new(extent: u32, buffer: u16) -> Result<Self, TileError> {
        // `extent + buffer` cannot overflow `u64`.
        if extent == 0 || extent as u64 + buffer as u64 > i32::MAX as u64 {
            return Err(TileError::InvalidExtent);
        }
        // `2 * buffer` cannot overflow: `buffer <= u16::MAX`, so the product fits `u32`.
        if 2 * (buffer as u32) >= extent {
            return Err(TileError::BufferTooLarge);
        }
        Ok(Self {
            extent: extent.cast_signed(),
            buffer: buffer as i32,
        })
    }

    /// The tile side length / per-tile output resolution: kept vertices land in `0..extent`.
    pub(crate) fn extent(self) -> u32 {
        self.extent.cast_unsigned()
    }

    /// [`Self::extent`] as the `i32` it is stored as.
    pub(crate) const fn extent_i32(self) -> i32 {
        self.extent
    }

    /// [`Self::buffer`] as the `i32` it is stored as.
    pub(crate) const fn buffer_i32(self) -> i32 {
        self.buffer
    }

    /// The buffer kept around every tile, in tile-space units.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "buffer is always in 0..=u16::MAX (it was built from a u16)"
    )]
    pub(crate) fn buffer(self) -> u16 {
        self.buffer as u16
    }

    /// The buffered box `(min, max)` of a located tile, as [`Self::tile_buffered_bounds`] returns it
    /// (already proved to fit `i32` when the tile was located).
    pub(crate) const fn box_of(self, loc: &Located) -> (Coord<i32>, Coord<i32>) {
        (
            shift(loc.core_lo, -self.buffer),
            shift(loc.core_hi, self.buffer),
        )
    }

    /// The origin (`tile · extent`) of the tile whose buffered box starts at `min`, as returned by
    /// [`Self::tile_buffered_bounds`] - which already proved `origin - buffer` fits `i32`.
    pub(crate) const fn origin_of(self, min: Coord<i32>) -> Coord<i32> {
        shift(min, self.buffer)
    }

    /// The closed integer bounds `(min, max)` of `tile`'s clip box (in output space), grown by
    /// `buffer` on each side. All arithmetic is checked; [`TileError::Overflow`] means the tile lies
    /// outside the representable range for this `extent`.
    pub(crate) fn tile_buffered_bounds(
        self,
        tile: TileId,
    ) -> Result<(Coord<i32>, Coord<i32>), TileError> {
        let base_x = tile.x.checked_mul(self.extent).ok_or(TileError::Overflow)?;
        let base_y = tile.y.checked_mul(self.extent).ok_or(TileError::Overflow)?;
        // Distance from the base corner to the far corner of the buffered box: extent - 1 + buffer.
        let reach = (self.extent - 1)
            .checked_add(self.buffer)
            .ok_or(TileError::Overflow)?;
        Ok((
            Coord {
                x: base_x.checked_sub(self.buffer).ok_or(TileError::Overflow)?,
                y: base_y.checked_sub(self.buffer).ok_or(TileError::Overflow)?,
            },
            Coord {
                x: base_x.checked_add(reach).ok_or(TileError::Overflow)?,
                y: base_y.checked_add(reach).ok_or(TileError::Overflow)?,
            },
        ))
    }

    /// The origin (`tile · extent`) of `tile`, with plain arithmetic: the caller has proved its box
    /// fits `i32`, as for [`Self::tile_box`].
    pub(crate) fn tile_origin(self, tile: TileId) -> Coord<i32> {
        Coord {
            x: tile.x * self.extent,
            y: tile.y * self.extent,
        }
    }

    /// The buffered box `(min, max)` of `tile`, with plain arithmetic: the caller has proved it fits
    /// `i32` (see [`Self::check_tiles`]), so unlike [`Self::tile_buffered_bounds`] it cannot fail.
    pub(crate) fn tile_box(self, tile: TileId) -> (Coord<i32>, Coord<i32>) {
        let base = Coord {
            x: tile.x * self.extent,
            y: tile.y * self.extent,
        };
        (
            shift(base, -self.buffer),
            shift(base, self.extent - 1 + self.buffer),
        )
    }

    /// Prove that every tile whose buffered box can meet the bounding box `[lo, hi]` has a box that
    /// fits `i32` (so [`Self::tile_box`], [`Self::locate_fit`] and a tile id from a walk over geometry
    /// inside it cannot overflow).
    ///
    /// The tiles form a rectangle, and a box grows with the tile index, so only the corner tiles need
    /// testing.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if a corner tile's box leaves the `i32` range.
    #[inline]
    pub(crate) fn check_tiles(self, lo: Coord<i32>, hi: Coord<i32>) -> Result<(), TileError> {
        // Away from the `i32` limits (the usual case) no box can overflow: the tiles lie within
        // `2^30 + extent + buffer < 2^31` of the origin. Each coordinate, biased by `2^30`, is below
        // `2^31` exactly when it is in `-2^30..2^30`, so one OR tests all four.
        if self.near_origin(lo, hi) {
            return Ok(());
        }
        self.check_tiles_exact(lo, hi, 0)
    }

    /// Whether the bounding box `[lo, hi]` lies so far from the `i32` limits that no tile meeting it
    /// can overflow, with room to spare: the tiles lie within `2^30 + extent + buffer < 2^31 - 1` of
    /// the origin. Each coordinate, biased by `2^30`, is below `2^31` exactly when it is in
    /// `-2^30..2^30`, so one OR tests all four.
    #[inline]
    fn near_origin(self, lo: Coord<i32>, hi: Coord<i32>) -> bool {
        let biased = |v: i32| v.cast_unsigned().wrapping_add(1 << 30);
        let in_range = biased(lo.x) | biased(lo.y) | biased(hi.x) | biased(hi.y);
        self.extent < 1 << 29 && in_range < 1 << 31
    }

    /// [`Self::check_tiles`] without the shortcut, with every box grown `margin` more units.
    fn check_tiles_exact(
        self,
        lo: Coord<i32>,
        hi: Coord<i32>,
        margin: i64,
    ) -> Result<(), TileError> {
        let (extent, buffer) = (i64::from(self.extent), i64::from(self.buffer));
        let (min, max) = (i64::from(i32::MIN), i64::from(i32::MAX));
        for (lo, hi) in [(lo.x, hi.x), (lo.y, hi.y)] {
            let first = (i64::from(lo) - buffer).div_euclid(extent);
            let last = (i64::from(hi) + buffer).div_euclid(extent);
            if first * extent - buffer - margin < min
                || last * extent + extent - 1 + buffer + margin > max
            {
                return Err(TileError::Overflow);
            }
        }
        Ok(())
    }

    /// [`Self::check_tiles`], and also that `p - origin` fits `i32` for every point `p` of `[lo, hi]`
    /// and the origin of every tile it checks: what a line run, whose vertices are lent in the local
    /// frame of a tile they only reach, needs.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if either fails.
    pub(crate) fn check_bounds(self, lo: Coord<i32>, hi: Coord<i32>) -> Result<(), TileError> {
        if self.near_origin_and_small(lo, hi) {
            return Ok(());
        }
        self.check_tiles(lo, hi)?;
        let (extent, buffer) = (i64::from(self.extent), i64::from(self.buffer));
        for (lo, hi) in [(lo.x, hi.x), (lo.y, hi.y)] {
            let first = (i64::from(lo) - buffer).div_euclid(extent);
            let last = (i64::from(hi) + buffer).div_euclid(extent);
            if i64::from(hi) - first * extent > i64::from(i32::MAX)
                || i64::from(lo) - last * extent < i64::from(i32::MIN)
            {
                return Err(TileError::Overflow);
            }
        }
        Ok(())
    }

    /// [`Self::check_bounds`], and also that every box it checks, grown one more unit, fits `i32`:
    /// what a polygon needs, whose pieces get synthetic corners just outside a tile's box.
    ///
    /// # Errors
    ///
    /// [`TileError::Overflow`] if any check fails.
    pub(crate) fn check_polygon_bounds(
        self,
        lo: Coord<i32>,
        hi: Coord<i32>,
    ) -> Result<(), TileError> {
        if self.near_origin_and_small(lo, hi) {
            return Ok(());
        }
        self.check_bounds(lo, hi)?;
        self.check_tiles_exact(lo, hi, 1)
    }

    /// [`Self::near_origin`], and `[lo, hi]` spans under `2^30` on each axis: then every point of it
    /// is also within `2^30 + extent + buffer + 1 < 2^31` of the origin of every tile it reaches, so
    /// every check of [`Self::check_polygon_bounds`] passes.
    #[inline]
    fn near_origin_and_small(self, lo: Coord<i32>, hi: Coord<i32>) -> bool {
        self.near_origin(lo, hi) && hi.x.abs_diff(lo.x) < 1 << 30 && hi.y.abs_diff(lo.y) < 1 << 30
    }

    /// Locate the tile owning `c`, a vertex inside bounds [`Self::check_tiles`] accepted, with its core
    /// and inner boxes precomputed (see [`Located`]): `min = base - buffer` and
    /// `max = base + extent - 1 + buffer` fit `i32`, and the core (`base .. base + extent - 1`) and
    /// inner (`base + buffer .. max - 2·buffer`) boxes follow by `± buffer`, so plain arithmetic
    /// cannot fail.
    pub(crate) fn locate_fit(self, c: Coord<i32>) -> Located {
        let owner = tile_of(c, self.extent);
        let (min, max) = self.tile_box(owner);
        Located {
            owner,
            core_lo: shift(min, self.buffer),
            core_hi: shift(max, -self.buffer),
            inner_lo: shift(min, 2 * self.buffer),
            inner_hi: shift(max, -2 * self.buffer),
        }
    }

    /// `log2(extent)` when the extent is a power of two (every tile size in use), so a tile index is a
    /// shift, and the coordinates `lo..=hi` whose tile boxes, buffer included, surely fit `i32`. (Not
    /// kept in the grid: it is passed by value everywhere, and fits a register as it is.)
    #[inline]
    const fn fast(self) -> Option<(u32, i32, i32)> {
        let extent = self.extent.cast_unsigned();
        if !extent.is_power_of_two() {
            return None;
        }
        // `extent + buffer` fits `i32` (checked when the grid was made), so these cannot overflow.
        let reach = self.extent + self.buffer;
        Some((extent.trailing_zeros(), i32::MIN + reach, i32::MAX - reach))
    }

    /// The tile whose inner box holds every vertex of `ring`, with its origin, and how many distinct
    /// vertices the ring has: what [`Self::inner_tile`] finds for its bounding box, without computing
    /// one. Only for a power-of-two extent and a ring far from the `i32` limits (the usual case), as
    /// [`ring_within`] takes it; `None` otherwise, or if some vertex is outside the inner box of the
    /// first vertex's tile: the caller takes the general path.
    ///
    /// The tile comes from the first vertex by a shift, and each vertex then costs one unsigned
    /// comparison per axis of its offset from the inner box's corner.
    #[inline]
    #[expect(
        clippy::cast_sign_loss,
        reason = "a span of the inner box, which is not empty"
    )]
    pub(crate) fn inner_tile_of_ring<V: Vertex>(
        self,
        ring: &[V],
    ) -> Option<(TileId, Coord<i32>, usize)> {
        let first = ring.first()?.position();
        let (shift, lo, hi) = self.fast()?;
        if first.x < lo || first.x > hi || first.y < lo || first.y > hi {
            return None;
        }
        let (tx, ty) = (first.x >> shift, first.y >> shift);
        let (x, y) = (tx << shift, ty << shift);
        // (Far from the limits, the inner box's corner fits `i32`.)
        let inner = Coord {
            x: x + self.buffer,
            y: y + self.buffer,
        };
        let n = ring_within(ring, inner, (self.extent - 1 - 2 * self.buffer) as u32)?;
        Some((TileId::new(tx, ty), Coord { x, y }, n))
    }

    /// The tile whose inner box (≥ `buffer` from every cell edge) holds the whole bounding box
    /// `[lo, hi]`, with its origin, if one does and its buffered box fits `i32`; one division per
    /// axis.
    #[expect(
        clippy::cast_sign_loss,
        reason = "offsets from a tile's corner, below, are non-negative"
    )]
    pub(crate) fn inner_tile(self, lo: Coord<i32>, hi: Coord<i32>) -> Option<(TileId, Coord<i32>)> {
        // The usual case: a power-of-two extent, and coordinates far from the `i32` limits, where
        // the tile box surely fits and a tile index is a shift.
        if let Some((shift, fast_lo, fast_hi)) = self.fast()
            && lo.x >= fast_lo
            && lo.y >= fast_lo
            && hi.x <= fast_hi
            && hi.y <= fast_hi
        {
            let (last, buffer) = ((self.extent - 1 - self.buffer) as u32, self.buffer as u32);
            let (tx, ty) = (lo.x >> shift, lo.y >> shift);
            let (x, y) = (tx << shift, ty << shift);
            // Offsets from the tile's corner, exact as `u32` (`hi ≥ lo ≥ corner`).
            let (ox, oy) = (lo.x.wrapping_sub(x) as u32, lo.y.wrapping_sub(y) as u32);
            let (ex, ey) = (hi.x.wrapping_sub(x) as u32, hi.y.wrapping_sub(y) as u32);
            return (ox >= buffer && oy >= buffer && ex <= last && ey <= last)
                .then_some((TileId::new(tx, ty), Coord { x, y }));
        }
        let (extent, buffer) = (i64::from(self.extent), i64::from(self.buffer));
        let axis = |lo: i32, hi: i32| {
            let (lo, hi) = (i64::from(lo), i64::from(hi));
            let base = lo.div_euclid(extent) * extent;
            let inside = lo >= base + buffer && hi <= base + extent - 1 - buffer;
            let fits = base - buffer >= i64::from(i32::MIN)
                && base + extent - 1 + buffer <= i64::from(i32::MAX);
            // `base` fits `i32` (its buffered box does), and so does `base / extent`.
            #[expect(clippy::cast_possible_truncation, reason = "checked to fit just above")]
            (inside && fits).then_some(((base / extent) as i32, base as i32))
        };
        let (tx, x) = axis(lo.x, hi.x)?;
        let (ty, y) = axis(lo.y, hi.y)?;
        Some((TileId::new(tx, ty), Coord { x, y }))
    }
}

/// How many distinct vertices `ring` has - its length, less a closing vertex repeating the first - if
/// it has at least three, no two consecutive the same (a ring closed twice included), and every one
/// in the square `lo ..= lo + span` on both axes; else `None`, for the general path.
///
/// An offset from `lo` is inside iff, as `u32` (so below zero wraps far past it), it is at most
/// `span`: one comparison per axis, and no branch per vertex.
#[inline]
pub(crate) fn ring_within<V: Vertex>(ring: &[V], lo: Coord<i32>, span: u32) -> Option<usize> {
    let first = ring.first()?.position();
    let n = ring.len() - usize::from(ring[ring.len() - 1].position() == first);
    if n < 3 || ring[n - 1].position() == first {
        return None;
    }
    let off = |v: i32, l: i32| v.wrapping_sub(l).cast_unsigned();
    let (mut prev, mut bad) = (ring[n - 1].position(), false);
    for v in &ring[..n] {
        let p = v.position();
        #[expect(
            clippy::needless_bitwise_bool,
            reason = "no branch per vertex: a ring is almost always inside"
        )]
        {
            bad |= (off(p.x, lo.x) > span) | (off(p.y, lo.y) > span) | (p == prev);
        }
        prev = p;
    }
    (!bad).then_some(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip_polyline::segment_intersects;

    /// [`Grid::inner_tile_of_ring`] finds exactly what [`Grid::inner_tile`] does for the ring's
    /// bounding box, wherever it applies: a power-of-two extent, a ring with three or more distinct
    /// vertices and no consecutive repeat, far from the `i32` limits.
    #[test]
    fn lone_ring_tile_agrees_with_the_bounding_box() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move |n: i64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            i64::try_from(state % u64::try_from(n).unwrap()).unwrap()
        };
        let mut found = 0;
        for case in 0..20_000 {
            let (extent, buffer) = [
                (16u32, 0u16),
                (16, 3),
                (64, 5),
                (4096, 64),
                (8, 3),
                (4096, 0),
            ][case % 6];
            let grid = Grid::new(extent, buffer).unwrap();
            // Rings of a few vertices near one tile's inner box, often straddling it.
            let e = i64::from(extent);
            let (cx, cy) = (next(7) - 3, next(7) - 3);
            let n = usize::try_from(3 + next(4)).unwrap();
            let mut ring: Vec<Coord<i32>> = (0..n)
                .map(|_| Coord {
                    x: i32::try_from(cx * e + next(e + 12) - 6).unwrap(),
                    y: i32::try_from(cy * e + next(e + 12) - 6).unwrap(),
                })
                .collect();
            if case % 3 == 0 {
                ring.push(ring[0]); // closed
            }
            let Some((tile, origin, vertices)) = grid.inner_tile_of_ring(&ring) else {
                continue;
            };
            let distinct = &ring[..vertices];
            assert_eq!(distinct.len(), n, "closing vertex dropped, only then");
            let lo = Coord {
                x: distinct.iter().map(|c| c.x).min().unwrap(),
                y: distinct.iter().map(|c| c.y).min().unwrap(),
            };
            let hi = Coord {
                x: distinct.iter().map(|c| c.x).max().unwrap(),
                y: distinct.iter().map(|c| c.y).max().unwrap(),
            };
            assert_eq!(
                grid.inner_tile(lo, hi),
                Some((tile, origin)),
                "{ring:?} e{extent} b{buffer}"
            );
            found += 1;
        }
        assert!(found > 1_000, "{found} rings found a tile");
    }

    /// The reverse: a ring the quick test declines, though it is distinct and inside one inner box,
    /// is declined only for a reason the general path handles (a repeat, or too few vertices).
    #[test]
    fn lone_ring_tile_declines_only_what_it_should() {
        let grid = Grid::new(4096, 64).unwrap();
        let c = |x, y| Coord { x, y };
        let inside = [c(100, 100), c(130, 100), c(130, 120), c(100, 120)];
        assert!(grid.inner_tile_of_ring(&inside).is_some());
        // Not the same tile, or in the buffer band.
        assert!(
            grid.inner_tile_of_ring(&[c(100, 100), c(5000, 100), c(130, 120)])
                .is_none()
        );
        assert!(
            grid.inner_tile_of_ring(&[c(100, 100), c(130, 100), c(130, 30)])
                .is_none()
        );
        // A repeated vertex, or two distinct ones, goes to the general path.
        assert!(
            grid.inner_tile_of_ring(&[c(100, 100), c(100, 100), c(130, 100), c(130, 120)])
                .is_none()
        );
        assert!(
            grid.inner_tile_of_ring(&[c(100, 100), c(130, 100), c(100, 100)])
                .is_none()
        );
        // A ring closed twice: the general path drops both closing vertices.
        assert!(
            grid.inner_tile_of_ring(&[inside[0], inside[1], inside[2], inside[0], inside[0]])
                .is_none()
        );
        // A closing vertex is not counted; far from the origin or off a power of two, the general path.
        assert_eq!(
            grid.inner_tile_of_ring(&[inside[0], inside[1], inside[2], inside[0]])
                .map(|r| r.2),
            Some(3)
        );
        assert!(
            grid.inner_tile_of_ring(&[
                c(i32::MAX - 5, 100),
                c(i32::MAX - 6, 100),
                c(i32::MAX - 6, 120)
            ])
            .is_none()
        );
        assert!(
            Grid::new(100, 5)
                .unwrap()
                .inner_tile_of_ring(&inside)
                .is_none()
        );
    }

    #[test]
    fn mul_div_floor_is_exact_past_i64() {
        // Spans of ~4·10⁹ (two far-apart full-i32 coordinates) overflow the i64 product.
        let reference = |n: i64, m: i64, d: i64| {
            i64::try_from((i128::from(n) * i128::from(m)).div_euclid(i128::from(d))).unwrap()
        };
        for (n, m, d) in [
            (4_000_000_000, 4_294_967_295, 4_294_967_296),
            (-3_999_999_999, 4_294_967_295, 4_000_000_000),
            (12, -7, 5),
        ] {
            assert_eq!(mul_div_floor(n, m, d), reference(n, m, d), "{n}·{m}/{d}");
        }
    }

    /// The previous routing: test every tile of the segment's buffered bounding box, row-major.
    fn scanned(grid: Grid, a: Coord<i32>, c: Coord<i32>) -> Result<Vec<TileId>, TileError> {
        let (extent, buffer) = (i64::from(grid.extent), i64::from(grid.buffer));
        let tile = |v: i32, d: i64| i32::try_from((i64::from(v) + d).div_euclid(extent)).unwrap();
        let mut out = Vec::new();
        for ty in tile(a.y.min(c.y), -buffer)..=tile(a.y.max(c.y), buffer) {
            for tx in tile(a.x.min(c.x), -buffer)..=tile(a.x.max(c.x), buffer) {
                let tile = TileId::new(tx, ty);
                let (min, max) = grid.tile_buffered_bounds(tile)?;
                if segment_intersects(a, c, min, max) {
                    out.push(tile);
                }
            }
        }
        Ok(out)
    }

    /// The lazy walk visits exactly the tiles (in the same order) that the bounding-box scan accepts,
    /// skipping the gap rows between unbuffered tiles; and [`Grid::check_bounds`] rejects exactly what
    /// the scan's tile boxes cannot hold, so a walk it accepted never meets an overflow.
    #[test]
    fn walk_matches_bounding_box_scan() {
        // A deterministic xorshift stream, reduced to `0..n`.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut below = |n: u32| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            i64::from(u32::try_from(state % u64::from(n)).unwrap())
        };
        let mut overflows = 0;
        for case in 0..20_000 {
            let extent: u32 = [1, 2, 3, 5, 7, 10, 25, 4096][case % 8];
            let buffer = u16::try_from(below(extent.div_ceil(2))).unwrap();
            let grid = Grid::new(extent, buffer).unwrap();
            // Spans of up to 40 tiles per axis, around an origin sometimes at the i32 limits.
            let span = extent * 40;
            let origin = match case % 5 {
                0 => i64::from(i32::MIN),
                1 => i64::from(i32::MAX) - i64::from(span),
                _ => -i64::from(span / 2),
            };
            let mut coord = || {
                let mut v = || i32::try_from(origin + below(span)).unwrap();
                Coord { x: v(), y: v() }
            };
            let (a, mut c) = (coord(), coord());
            // Bias towards axis-parallel and near-vertical segments, the walk's degenerate rows.
            match below(4) {
                0 => c.y = a.y,
                1 => c.x = a.x,
                2 => c.x = i32::try_from(i64::from(a.x) + below(3) - 1).unwrap_or(a.x),
                _ => {}
            }
            let (lo, hi) = (
                Coord {
                    x: a.x.min(c.x),
                    y: a.y.min(c.y),
                },
                Coord {
                    x: a.x.max(c.x),
                    y: a.y.max(c.y),
                },
            );
            // Rejected bounds are exactly those with a tile box (or tile id) beyond `i32`.
            let reference = scanned(grid, a, c);
            let rejected = grid.check_tiles(lo, hi).is_err();
            assert_eq!(
                rejected,
                reference.is_err(),
                "{extent}/{buffer} {a:?}-{c:?}"
            );
            if rejected {
                overflows += 1;
                continue;
            }
            let reference = reference.expect("accepted bounds hold every tile box");
            let walk: Vec<TileId> = SegmentTiles::new(grid, a, c).collect();
            assert_eq!(walk, reference, "{extent}/{buffer} {a:?}-{c:?}");
            for &tile in &walk {
                assert_eq!(
                    grid.tile_box(tile),
                    grid.tile_buffered_bounds(tile).unwrap()
                );
            }
        }
        assert!(
            overflows > 100,
            "the i32 limits are exercised ({overflows})"
        );
    }
}
