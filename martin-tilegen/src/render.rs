//! Renders one feature into tile pieces for every zoom of its layer, pushed straight into a sort buffer.
//! Per zoom: size filter, simplification, quantization to the zoom grid, slicing, one record per tile.

use std::ops::{Range, RangeInclusive};

use geo_types::{Coord, LineString, Polygon};
use map_tile_toolkit::{PolygonSlicerAll, SlicerAll, TileError, signed_area_2x};
use martin_tile_utils::TileCoord;

use crate::props::{KeyId, PropRef};
use crate::record::{EncodedProps, Geom, Vertex, encode};
use crate::simplify::Simplifier;
use crate::{LayerGrid, Seq, SortBuffer, SortKey, TileGenError, TileGenResult, TileOrder};

/// Simplification and size thresholds are in pixels of a 256-pixel tile, as in Planetiler.
const TILE_PIXELS: f64 = 256.0;

/// A feature in Web Mercator unit coordinates (see [`project`](crate::project)).
pub struct Feature<'a> {
    pub id: Option<u64>,
    pub geom: FeatureGeom<'a>,
    pub props: &'a [(KeyId, PropRef<'a>)],
}

pub enum FeatureGeom<'a> {
    Points(&'a [Coord<f64>]),
    Lines(&'a [LineString<f64>]),
    Polygons(&'a [Polygon<f64>]),
}

#[derive(Clone, Debug)]
pub struct RenderLayer {
    /// The layer byte of the sort key.
    pub index: u8,
    pub zooms: RangeInclusive<u8>,
    pub grid: LayerGrid,
    /// `false` keeps whole features in every tile they touch, like `ST_AsMVTGeom(..., clip_geom => false)`.
    pub clip: bool,
    /// RDP tolerance in pixels below the layer's max zoom, and at it.
    pub simplify: (f64, f64),
    /// Lines and polygons whose bounding box is smaller than this many pixels are dropped, below the
    /// max zoom and at it. Size only shrinks with zoom, so a dropped feature stays dropped.
    pub min_size: (f64, f64),
    /// Only tiles intersecting these unit-coordinate bounds (`[min_x, min_y, max_x, max_y]`) are kept.
    pub bounds: Option<[f64; 4]>,
}

impl RenderLayer {
    /// Planetiler's defaults: simplify 0.1 px (1/16 px at the max zoom), drop below 1 px (1/16 px).
    pub fn new(index: u8, zooms: RangeInclusive<u8>, grid: LayerGrid) -> TileGenResult<Self> {
        // The zoom grid must fit `i32` with room for buffers and features crossing the antimeridian.
        let fits = u64::from(grid.extent)
            .checked_shl(u32::from(*zooms.end()))
            .is_some_and(|s| s <= 1 << 30);
        if !fits || zooms.is_empty() || grid.buffer >= grid.extent / 2 {
            return Err(TileGenError::InvalidLayer {
                index,
                extent: grid.extent,
                max_zoom: *zooms.end(),
            });
        }
        Ok(Self {
            index,
            zooms,
            grid,
            clip: true,
            simplify: (0.1, 0.0625),
            min_size: (1.0, 0.0625),
            bounds: None,
        })
    }

    fn contains(&self, zoom: u8, x: u32, y: u32) -> bool {
        let (xs, ys) = self.tile_bounds(zoom);
        xs.contains(&x) && ys.contains(&y)
    }

    /// The tile columns and rows of `zoom` that [`bounds`](Self::bounds) allows.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to the grid"
    )]
    fn tile_bounds(&self, zoom: u8) -> (Range<u32>, Range<u32>) {
        let side = scale(zoom);
        let Some([x0, y0, x1, y1]) = self.bounds else {
            let all = 0..1 << zoom;
            return (all.clone(), all);
        };
        let tile = |v: f64| (v * side).floor().clamp(0.0, side - 1.0) as u32;
        (tile(x0)..tile(x1) + 1, tile(y0)..tile(y1) + 1)
    }

    fn at_max(&self, zoom: u8, (below, at): (f64, f64)) -> f64 {
        if zoom == *self.zooms.end() { at } else { below }
    }
}

/// Per worker; every buffer is reused across features.
#[derive(Default)]
pub struct Renderer {
    slicers: Vec<(LayerGrid, SlicerAll)>,
    poly_slicers: Vec<(LayerGrid, PolygonSlicerAll)>,
    props: EncodedProps,
    simplifier: Simplifier,
    kept: Vec<usize>,
    /// The quantized feature in the zoom's global grid: ring counts per polygon, line or ring lengths,
    /// and vertices.
    polys: Vec<u32>,
    parts: Vec<u32>,
    vertices: Vec<Coord<i32>>,
    /// One tile's piece, shaped the same with tile-local vertices.
    piece: PieceBuf,
    points: Vec<(i32, i32, Vertex)>,
    /// Fill runs as `(x_start, x_end, y)`, and the tile-id ranges they become.
    runs: Vec<(i32, i32, i32)>,
    ranges: Vec<Range<u64>>,
    /// Features skipped at a zoom because the slicer could not handle them.
    pub slice_errors: u64,
}

impl Renderer {
    pub fn render(
        &mut self,
        order: TileOrder,
        layer: &RenderLayer,
        seq: Seq,
        feature: &Feature<'_>,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.props.clear();
        for &(key, value) in feature.props {
            self.props.push(key, value);
        }
        let ctx = Ctx {
            order,
            layer,
            seq,
            id: feature.id,
        };
        match feature.geom {
            FeatureGeom::Points(points) => {
                let shift = world_shift(points.iter());
                for zoom in layer.zooms.clone().rev() {
                    self.render_points(&ctx, zoom, points, shift, out)?;
                }
            }
            FeatureGeom::Lines(lines) => {
                self.render_zooms(
                    &ctx,
                    lines.iter().flat_map(|l| &l.0),
                    out,
                    |r, zoom, shift, out| r.render_lines(&ctx, zoom, lines, shift, out),
                )?;
            }
            FeatureGeom::Polygons(polygons) => {
                let exteriors = polygons.iter().flat_map(|p| &p.exterior().0);
                self.render_zooms(&ctx, exteriors, out, |r, zoom, shift, out| {
                    r.render_polygons(&ctx, zoom, polygons, shift, out)
                })?;
            }
        }
        Ok(())
    }

    /// Max zoom first, stopping once the feature is below the minimum size (size only shrinks with zoom).
    /// A zoom the slicer cannot handle is counted and skipped; lower zooms span fewer tiles and may work.
    fn render_zooms<'c>(
        &mut self,
        ctx: &Ctx<'_>,
        coords: impl Iterator<Item = &'c Coord<f64>>,
        out: &mut SortBuffer<'_>,
        mut render: impl FnMut(&mut Self, u8, f64, &mut SortBuffer<'_>) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let Some((lo, hi)) = bbox(coords) else {
            return Ok(());
        };
        let shift = world_shift([lo, hi].iter());
        for zoom in ctx.layer.zooms.clone().rev() {
            let size = (hi.x - lo.x).max(hi.y - lo.y) * TILE_PIXELS * scale(zoom);
            if size < ctx.layer.at_max(zoom, ctx.layer.min_size) {
                break;
            }
            match render(self, zoom, shift, out) {
                Err(TileGenError::Slice(_)) => self.slice_errors += 1,
                other => other?,
            }
        }
        Ok(())
    }

    /// Simplifies (RDP is scale-invariant, so in unit coordinates with the tolerance scaled to them),
    /// quantizes and appends `line` without consecutive duplicates; returns how many vertices it kept.
    fn push_quantized(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        line: &LineString<f64>,
        shift: f64,
    ) -> TileGenResult<usize> {
        let world = scale(zoom) * f64::from(ctx.layer.grid.extent);
        let epsilon = ctx.layer.at_max(zoom, ctx.layer.simplify) / (TILE_PIXELS * scale(zoom));
        self.kept.clear();
        self.simplifier.simplify(&line.0, epsilon, &mut self.kept);
        let start = self.vertices.len();
        for &i in &self.kept {
            let [x, y] = quantize(line.0[i], shift, world)?;
            let c = Coord { x, y };
            if self.vertices.len() == start || self.vertices.last() != Some(&c) {
                self.vertices.push(c);
            }
        }
        Ok(self.vertices.len() - start)
    }

    fn render_points(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        points: &[Coord<f64>],
        shift: f64,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let grid = ctx.layer.grid;
        let (extent, buffer) = (to_i32(grid.extent)?, to_i32(grid.buffer)?);
        let world = scale(zoom) * f64::from(grid.extent);
        self.points.clear();
        for c in points {
            let [x, y] = quantize(*c, shift, world)?;
            let (tx, ty) = (x.div_euclid(extent), y.div_euclid(extent));
            let (lx, ly) = (x - tx * extent, y - ty * extent);
            // A point near an edge also lands in the neighbor's buffer.
            let near = |l: i32| {
                [
                    (l < buffer).then_some(-1),
                    Some(0),
                    (l >= extent - buffer).then_some(1),
                ]
            };
            for dx in near(lx).into_iter().flatten() {
                for dy in near(ly).into_iter().flatten() {
                    self.points
                        .push((tx + dx, ty + dy, [lx - dx * extent, ly - dy * extent]));
                }
            }
        }
        self.points.sort_by_key(|&(tx, ty, _)| (tx, ty));
        let mut rest = self.points.as_slice();
        while let Some(&(tx, ty, _)) = rest.first() {
            let len = rest.iter().take_while(|p| (p.0, p.1) == (tx, ty)).count();
            self.piece.clear();
            self.piece.vertices.extend(rest[..len].iter().map(|p| p.2));
            rest = &rest[len..];
            ctx.push(
                zoom,
                tx,
                ty,
                &self.props,
                Geom::Points(&self.piece.vertices),
                out,
            )?;
        }
        Ok(())
    }

    fn render_lines(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        lines: &[LineString<f64>],
        shift: f64,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.parts.clear();
        self.vertices.clear();
        for line in lines {
            let start = self.vertices.len();
            match self.push_quantized(ctx, zoom, line, shift)? {
                0 | 1 => self.vertices.truncate(start),
                n => self.parts.push(len32(n)?),
            }
        }

        let (parts, vertices) = (
            std::mem::take(&mut self.parts),
            std::mem::take(&mut self.vertices),
        );
        let result = match sole_tile(&vertices, ctx.layer.grid)? {
            Some(tile) => self.push_whole(ctx, zoom, tile, (&[], &parts, &vertices), out),
            None => self.slice_lines(ctx, zoom, &parts, &vertices, out),
        };
        (self.parts, self.vertices) = (parts, vertices);
        result
    }

    fn slice_lines(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        parts: &[u32],
        vertices: &[Coord<i32>],
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let grid = ctx.layer.grid;
        let slicer = self.slicer(grid)?;
        slicer.clear();
        let mut rest = vertices;
        for &len in parts {
            let (line, tail) = rest.split_at(len as usize);
            rest = tail;
            slicer.add_feature(line)?;
        }
        let slicer = &self
            .slicers
            .iter()
            .find(|(g, _)| same_grid(*g, grid))
            .expect("created above")
            .1;
        let extent = to_i32(grid.extent)?;
        for tile in slicer.iter_tiles() {
            let id = tile.tile_id();
            if ctx.layer.clip {
                self.piece.clear();
                for polyline in tile.iter_features().flat_map(|f| f.iter_polylines()) {
                    self.piece.parts.push(len32(polyline.len())?);
                    self.piece
                        .vertices
                        .extend(polyline.iter().map(|c| [c.x, c.y]));
                }
            } else {
                self.piece
                    .set_whole((&[], parts, vertices), (id.x * extent, id.y * extent));
            }
            ctx.push(zoom, id.x, id.y, &self.props, self.piece.lines(), out)?;
        }
        Ok(())
    }

    fn render_polygons(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        polygons: &[Polygon<f64>],
        shift: f64,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.polys.clear();
        self.parts.clear();
        self.vertices.clear();
        for polygon in polygons {
            let first_ring = self.parts.len();
            for (i, ring) in std::iter::once(polygon.exterior())
                .chain(polygon.interiors())
                .enumerate()
            {
                let start = self.vertices.len();
                let mut n = self.push_quantized(ctx, zoom, ring, shift)?;
                if n > 1 && self.vertices[start] == self.vertices[start + n - 1] {
                    self.vertices.pop();
                    n -= 1;
                }
                // Exteriors get a positive area in y-down tile coordinates and holes a negative one,
                // the MVT and MLT winding; a ring collapsed by quantization has none and is dropped.
                let area = signed_area_2x(&self.vertices[start..]);
                if n < 3 || area == 0 {
                    self.vertices.truncate(start);
                    if i == 0 {
                        break;
                    }
                    continue;
                }
                if (area > 0) == (i > 0) {
                    self.vertices[start..].reverse();
                }
                self.parts.push(len32(n)?);
            }
            if self.parts.len() > first_ring {
                self.polys.push(len32(self.parts.len() - first_ring)?);
            }
        }
        let (polys, parts, vertices) = (
            std::mem::take(&mut self.polys),
            std::mem::take(&mut self.parts),
            std::mem::take(&mut self.vertices),
        );
        let result = match sole_tile(&vertices, ctx.layer.grid)? {
            Some(tile) => self.push_whole(ctx, zoom, tile, (&polys, &parts, &vertices), out),
            None => self.slice_polygons(ctx, zoom, &polys, &parts, &vertices, out),
        };
        (self.polys, self.parts, self.vertices) = (polys, parts, vertices);
        result
    }

    /// A feature in one tile is that tile's piece, unchanged but for the tile-local frame.
    fn push_whole(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        (tx, ty): (i32, i32),
        feature: Rings<'_>,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let extent = to_i32(ctx.layer.grid.extent)?;
        self.piece.set_whole(feature, (tx * extent, ty * extent));
        let geom = if feature.0.is_empty() {
            self.piece.lines()
        } else {
            self.piece.polygons()
        };
        ctx.push(zoom, tx, ty, &self.props, geom, out)
    }

    fn slice_polygons(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        polys: &[u32],
        parts: &[u32],
        vertices: &[Coord<i32>],
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        if polys.is_empty() {
            return Ok(());
        }
        let grid = ctx.layer.grid;
        let extent = to_i32(grid.extent)?;
        let mut rings = Vec::with_capacity(parts.len());
        let mut rest = vertices;
        for &len in parts {
            let (ring, tail) = rest.split_at(len as usize);
            rings.push(ring);
            rest = tail;
        }
        let mut rest = rings.as_slice();
        let polygons = polys.iter().map(|&count| {
            let (polygon, tail) = rest.split_at(count as usize);
            rest = tail;
            polygon
        });
        let pos = self.poly_slicer(grid)?;
        let slicer = &mut self.poly_slicers[pos].1;
        slicer.clear();
        slicer.add_feature(polygons)?;
        let Some(feature) = self.poly_slicers[pos].1.iter_features().next() else {
            return Ok(());
        };
        for tile in feature.iter_tiles() {
            let id = tile.tile_id();
            if ctx.layer.clip {
                self.piece.clear();
                for polygon in tile.iter_polygons() {
                    let first = self.piece.parts.len();
                    for ring in polygon.iter_rings() {
                        // The slicer closes rings; records store them open.
                        let open = ring
                            .vertices()
                            .split_last()
                            .map_or(&[][..], |(_, open)| open);
                        self.piece.parts.push(len32(open.len())?);
                        self.piece.vertices.extend(open.iter().map(|c| [c.x, c.y]));
                    }
                    self.piece
                        .polys
                        .push(len32(self.piece.parts.len() - first)?);
                }
            } else {
                self.piece
                    .set_whole((polys, parts, vertices), (id.x * extent, id.y * extent));
            }
            ctx.push(zoom, id.x, id.y, &self.props, self.piece.polygons(), out)?;
        }
        self.runs.clear();
        self.runs.extend(
            feature
                .iter_fill_runs()
                .map(|run| (run.x.start, run.x.end, run.y)),
        );
        self.push_fills(ctx, zoom, (polys, parts, vertices), out)
    }

    /// Rows with the same column span merge into rectangles, which become contiguous id ranges (or, for
    /// an unclipped layer, a copy of the whole feature in each tile).
    fn push_fills(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        feature: Rings<'_>,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.runs.sort_unstable();
        let side = 1i32 << zoom;
        let extent = to_i32(ctx.layer.grid.extent)?;
        let (allowed_x, allowed_y) = ctx.layer.tile_bounds(zoom);
        let runs = std::mem::take(&mut self.runs);
        let mut rest = runs.as_slice();
        let mut result = Ok(());
        while let Some(&(x0, x1, y0)) = rest.first()
            && result.is_ok()
        {
            let rows = rest
                .iter()
                .zip(y0..)
                .take_while(|(r, y)| (r.0, r.1, r.2) == (x0, x1, *y))
                .count();
            rest = &rest[rows..];
            let ys = clip(
                y0..y0.saturating_add(i32::try_from(rows).unwrap_or(i32::MAX)),
                0..side,
                &allowed_y,
            );
            // A feature past the antimeridian covers wrapped columns.
            let mut x = x0;
            while x < x1 && result.is_ok() {
                let world = x.div_euclid(side) * side;
                let end = x1.min(world + side);
                let xs = clip(x - world..end - world, 0..side, &allowed_x);
                x = end;
                if !xs.is_empty() && !ys.is_empty() {
                    result = self.push_rect(ctx, zoom, (xs, ys.clone()), feature, extent, out);
                }
            }
        }
        self.runs = runs;
        result
    }

    fn push_rect(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        (xs, ys): (Range<u32>, Range<u32>),
        feature: Rings<'_>,
        extent: i32,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        if ctx.layer.clip {
            self.ranges.clear();
            ctx.order.fill_ranges(zoom, xs, ys, &mut self.ranges)?;
            for range in &self.ranges {
                ctx.push_id(
                    range.start,
                    &self.props,
                    Geom::FillRange { end: range.end },
                    out,
                )?;
            }
            return Ok(());
        }
        for ty in ys {
            for tx in xs.clone() {
                let (tx, ty) = (to_i32(tx)?, to_i32(ty)?);
                self.piece.set_whole(feature, (tx * extent, ty * extent));
                ctx.push(zoom, tx, ty, &self.props, self.piece.polygons(), out)?;
            }
        }
        Ok(())
    }

    fn poly_slicer(&mut self, grid: LayerGrid) -> TileGenResult<usize> {
        if let Some(pos) = self
            .poly_slicers
            .iter()
            .position(|(g, _)| same_grid(*g, grid))
        {
            return Ok(pos);
        }
        let buffer = u16::try_from(grid.buffer).map_err(|_too_large| TileError::BufferTooLarge)?;
        self.poly_slicers
            .push((grid, PolygonSlicerAll::new(grid.extent, buffer)?));
        Ok(self.poly_slicers.len() - 1)
    }

    fn slicer(&mut self, grid: LayerGrid) -> TileGenResult<&mut SlicerAll> {
        if let Some(pos) = self.slicers.iter().position(|(g, _)| same_grid(*g, grid)) {
            return Ok(&mut self.slicers[pos].1);
        }
        let buffer = u16::try_from(grid.buffer).map_err(|_too_large| TileError::BufferTooLarge)?;
        self.slicers
            .push((grid, SlicerAll::new(grid.extent, buffer)?));
        Ok(&mut self.slicers.last_mut().expect("just pushed").1)
    }
}

struct Ctx<'a> {
    order: TileOrder,
    layer: &'a RenderLayer,
    seq: Seq,
    id: Option<u64>,
}

impl Ctx<'_> {
    /// Tiles past the antimeridian wrap around; tiles past the poles (only ever buffer) are dropped.
    fn push(
        &self,
        zoom: u8,
        tx: i32,
        ty: i32,
        props: &EncodedProps,
        geom: Geom<'_>,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let side = 1i32 << zoom;
        let (Ok(x), Ok(y)) = (u32::try_from(tx.rem_euclid(side)), u32::try_from(ty)) else {
            return Ok(());
        };
        if y >= side.cast_unsigned() || !self.layer.contains(zoom, x, y) {
            return Ok(());
        }
        self.push_id(
            self.order.tile_id(TileCoord::new_unchecked(zoom, x, y))?,
            props,
            geom,
            out,
        )
    }

    fn push_id(
        &self,
        tile_id: u64,
        props: &EncodedProps,
        geom: Geom<'_>,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        out.push_with(SortKey::new(tile_id, self.layer.index, self.seq), |buf| {
            encode(buf, self.id, props, geom);
        })
    }
}

/// A quantized feature in its zoom's global grid: ring counts per polygon, line or ring lengths, vertices.
type Rings<'a> = (&'a [u32], &'a [u32], &'a [Coord<i32>]);

/// One tile's piece of a feature, shaped like [`Rings`] with tile-local vertices.
#[derive(Default)]
struct PieceBuf {
    polys: Vec<u32>,
    parts: Vec<u32>,
    vertices: Vec<Vertex>,
}

impl PieceBuf {
    fn clear(&mut self) {
        self.polys.clear();
        self.parts.clear();
        self.vertices.clear();
    }

    /// The whole feature, unclipped, in the frame of the tile at `origin`.
    fn set_whole(&mut self, (polys, parts, vertices): Rings<'_>, (ox, oy): (i32, i32)) {
        self.clear();
        self.polys.extend_from_slice(polys);
        self.parts.extend_from_slice(parts);
        self.vertices
            .extend(vertices.iter().map(|c| [c.x - ox, c.y - oy]));
    }

    fn lines(&self) -> Geom<'_> {
        Geom::Lines {
            parts: &self.parts,
            vertices: &self.vertices,
        }
    }

    fn polygons(&self) -> Geom<'_> {
        Geom::Polygons {
            polygons: &self.polys,
            rings: &self.parts,
            vertices: &self.vertices,
        }
    }
}

/// `range` within the grid `0..side` and the allowed tiles, as tile indexes.
fn clip(range: Range<i32>, grid: Range<i32>, allowed: &Range<u32>) -> Range<u32> {
    let lo = u32::try_from(range.start.max(grid.start))
        .unwrap_or(0)
        .max(allowed.start);
    let hi = u32::try_from(range.end.min(grid.end))
        .unwrap_or(0)
        .min(allowed.end);
    lo..hi.max(lo)
}

/// The tile a quantized feature lies in when it keeps clear of every neighbor's buffer, as most
/// features do at most zooms: no slicer is needed to know it reaches that tile alone, whole.
fn sole_tile(vertices: &[Coord<i32>], grid: LayerGrid) -> TileGenResult<Option<(i32, i32)>> {
    let (extent, buffer) = (to_i32(grid.extent)?, i64::from(grid.buffer));
    let Some(first) = vertices.first() else {
        return Ok(None);
    };
    let tile = (first.x.div_euclid(extent), first.y.div_euclid(extent));
    // Strictly inside, as a vertex on a buffer's edge belongs to the neighbor too.
    let inside = |v: i32, t: i32| {
        let local = i64::from(v) - i64::from(t) * i64::from(extent);
        local > buffer && local < i64::from(extent) - buffer
    };
    let sole = vertices
        .iter()
        .all(|c| inside(c.x, tile.0) && inside(c.y, tile.1));
    Ok(sole.then_some(tile))
}

fn len32(n: usize) -> TileGenResult<u32> {
    u32::try_from(n).map_err(|_overflow| TileGenError::RecordTooLarge(n))
}

fn same_grid(a: LayerGrid, b: LayerGrid) -> bool {
    (a.extent, a.buffer) == (b.extent, b.buffer)
}

fn scale(zoom: u8) -> f64 {
    f64::from(1u32 << zoom)
}

fn to_i32(v: u32) -> TileGenResult<i32> {
    i32::try_from(v).map_err(|_overflow| TileGenError::Slice(TileError::Overflow))
}

/// Rounds to the zoom grid; `shift` moves features lying wholly outside the world back into it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "range-checked before the cast"
)]
fn quantize(c: Coord<f64>, shift: f64, world: f64) -> TileGenResult<Vertex> {
    let (x, y) = (((c.x + shift) * world).round(), (c.y * world).round());
    let range = f64::from(i32::MIN)..=f64::from(i32::MAX);
    if range.contains(&x) && range.contains(&y) {
        Ok([x as i32, y as i32])
    } else {
        Err(TileGenError::Slice(TileError::Overflow))
    }
}

/// Whole world widths that bring a feature lying entirely outside `[0, 1)` back into it, e.g. data drawn
/// at longitudes 181..190 to avoid splitting Fiji. A feature straddling the edge stays and is wrapped
/// per tile instead.
fn world_shift<'a>(xs: impl Iterator<Item = &'a Coord<f64>>) -> f64 {
    let (lo, hi) = xs.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), c| {
        (lo.min(c.x), hi.max(c.x))
    });
    if lo >= 1.0 || hi < 0.0 {
        -lo.floor()
    } else {
        0.0
    }
}

fn bbox<'a>(mut coords: impl Iterator<Item = &'a Coord<f64>>) -> Option<(Coord<f64>, Coord<f64>)> {
    let first = *coords.next()?;
    Some(coords.fold((first, first), |(lo, hi), c| {
        (
            Coord {
                x: lo.x.min(c.x),
                y: lo.y.min(c.y),
            },
            Coord {
                x: hi.x.max(c.x),
                y: hi.y.max(c.y),
            },
        )
    }))
}

#[cfg(test)]
mod tests {
    use geo_types::coord;

    use super::*;
    use crate::record::{GeomBuf, GeomKind, Record};
    use crate::{SortConfig, Sorter};

    const GRID: LayerGrid = LayerGrid {
        extent: 256,
        buffer: 8,
    };

    /// Renders and returns `(z/x/y, kind, decoded geometry)` per record, in sorted order.
    fn render(layer: &RenderLayer, geom: FeatureGeom<'_>) -> Vec<(String, GeomKind, GeomBuf)> {
        let mut out: Vec<_> = render_in(TileOrder::Tms, layer, geom)
            .into_iter()
            .map(|(id, kind, geom)| {
                (
                    format!("{:#}", TileOrder::Tms.tile_coord(id).unwrap()),
                    kind,
                    geom,
                )
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Renders and returns `(tile id, kind, decoded geometry)` per record.
    fn render_in(
        order: TileOrder,
        layer: &RenderLayer,
        geom: FeatureGeom<'_>,
    ) -> Vec<(u64, GeomKind, GeomBuf)> {
        let dir = tempfile::tempdir().unwrap();
        let sorter = Sorter::new(SortConfig {
            temp_dirs: vec![dir.path().to_path_buf()],
            buffer_bytes: 1 << 20,
            max_fan_in: 8,
            read_buffer_bytes: 4096,
        })
        .unwrap();
        let mut buffer = sorter.buffer();
        let feature = Feature {
            id: Some(1),
            geom,
            props: &[],
        };
        Renderer::default()
            .render(order, layer, Seq::default(), &feature, &mut buffer)
            .unwrap();
        buffer.finish().unwrap();
        let mut merger = sorter.merge().unwrap();
        let mut out = Vec::new();
        while let Some((key, bytes)) = merger.next().unwrap() {
            let record = Record::decode(bytes).unwrap();
            let mut geom = GeomBuf::default();
            record.append_geometry(&mut geom).unwrap();
            out.push((key.tile_id(), record.kind, geom));
        }
        out
    }

    /// Every tile a polygon reaches: edge pieces, plus the tiles of each fill range.
    fn polygon_tiles(
        order: TileOrder,
        layer: &RenderLayer,
        polygons: &[Polygon<f64>],
    ) -> (Vec<(u8, u32, u32)>, usize) {
        let records = render_in(order, layer, FeatureGeom::Polygons(polygons));
        let mut tiles = Vec::new();
        for (id, kind, _) in &records {
            let ids = match kind {
                GeomKind::FillRange { end } => *id..*end,
                GeomKind::Polygon => *id..*id + 1,
                other @ (GeomKind::Point | GeomKind::Line | GeomKind::Fill) => {
                    panic!("unexpected {other:?}")
                }
            };
            for id in ids {
                let c = order.tile_coord(id).unwrap();
                tiles.push((c.z(), c.x(), c.y()));
            }
        }
        tiles.sort_unstable();
        (tiles, records.len())
    }

    fn unit(lon: f64, lat: f64) -> Coord<f64> {
        let mut c = [coord! { x: lon, y: lat }];
        crate::project::from_lonlat(&mut c);
        c[0]
    }

    #[test]
    fn point_near_an_edge_reaches_the_neighbor_buffer() {
        let layer = RenderLayer::new(0, 1..=1, GRID).unwrap();
        // Just east of the center: tile 1/1/0 and 1/1/1, and within 8 units of x=256 so also the west tiles.
        let tiles = render(
            &layer,
            FeatureGeom::Points(&[coord! { x: 0.5 + 1.0 / 512.0, y: 0.5 }]),
        );
        let names: Vec<_> = tiles.iter().map(|t| t.0.as_str()).collect();
        assert_eq!(names, ["1/0/0", "1/0/1", "1/1/0", "1/1/1"]);
        let local: Vec<_> = tiles.iter().map(|t| t.2.vertices[0]).collect();
        assert_eq!(local, [[257, 256], [257, 0], [1, 256], [1, 0]]);
    }

    #[test]
    fn line_is_sliced_per_zoom_and_stops_when_too_small() {
        let layer = RenderLayer::new(0, 0..=3, GRID).unwrap();
        // 0.7 px long at z0, so dropped there; crosses the central meridian, so two tiles per zoom.
        let line = LineString::from(vec![unit(-0.5, 10.0), unit(0.5, 10.0)]);
        let tiles = render(&layer, FeatureGeom::Lines(std::slice::from_ref(&line)));
        let zooms: Vec<_> = tiles
            .iter()
            .map(|t| t.0.split('/').next().unwrap())
            .collect();
        assert_eq!(zooms, ["1", "1", "2", "2", "3", "3"]);
        assert!(tiles.iter().all(|t| t.1 == GeomKind::Line));
    }

    #[test]
    fn long_dense_diagonal_is_sliced_whole_at_z14() {
        let layer = RenderLayer::new(
            0,
            14..=14,
            LayerGrid {
                extent: 4096,
                buffer: 64,
            },
        )
        .unwrap();
        // More vertices than 16 bits index, zigzagging so none simplify away, 40 degrees diagonally.
        let line: LineString<f64> = (0..100_000_u32)
            .map(|i| {
                let t = f64::from(i) / 2500.0;
                unit(t, t + if i % 2 == 0 { 0.001 } else { 0.0 })
            })
            .collect();
        let tiles = render(&layer, FeatureGeom::Lines(std::slice::from_ref(&line)));
        // About 1800 tiles across and up, at least one per column crossed.
        assert!(tiles.len() > 1800, "{}", tiles.len());
        let vertices: usize = tiles.iter().map(|t| t.2.vertices.len()).sum();
        assert!(vertices >= 100_000, "{vertices}");
    }

    #[test]
    fn simplification_drops_vertices_at_low_zoom() {
        let mut layer = RenderLayer::new(0, 0..=8, GRID).unwrap();
        layer.min_size = (0.0, 0.0);
        let zigzag: Vec<_> = (0..=100)
            .map(|i| unit(f64::from(i) * 0.01, f64::from(i % 2) * 0.001))
            .collect();
        let tiles = render(&layer, FeatureGeom::Lines(&[LineString::from(zigzag)]));
        let z0 = tiles.iter().find(|t| t.0.starts_with("0/")).unwrap();
        assert_eq!(
            z0.2.vertices.len(),
            2,
            "the zigzag is far below a pixel at z0"
        );
        let z8: usize = tiles
            .iter()
            .filter(|t| t.0.starts_with("8/"))
            .map(|t| t.2.vertices.len())
            .sum();
        assert!(z8 > 50, "at z8 the zigzag is visible: {z8}");
    }

    #[test]
    fn features_past_the_antimeridian_move_into_the_world() {
        let layer = RenderLayer::new(0, 2..=2, GRID).unwrap();
        let tiles = render(&layer, FeatureGeom::Points(&[unit(200.0, 10.0)]));
        assert_eq!(
            tiles.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
            ["2/0/1"]
        );
    }

    #[test]
    fn wrapped_buffer_piece_lands_on_the_other_side() {
        let layer = RenderLayer::new(0, 1..=1, GRID).unwrap();
        let tiles = render(
            &layer,
            FeatureGeom::Points(&[coord! { x: 1.0 / 1024.0, y: 0.25 }]),
        );
        assert_eq!(
            tiles.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
            ["1/0/0", "1/1/0"]
        );
        let wrapped = tiles.iter().find(|t| t.0 == "1/1/0").unwrap();
        assert_eq!(wrapped.2.vertices, [[257, 128]]);
    }

    #[test]
    fn unclipped_lines_keep_the_whole_feature_per_tile() {
        let mut layer = RenderLayer::new(0, 1..=1, GRID).unwrap();
        layer.clip = false;
        let line = LineString::from(vec![coord! { x: 0.1, y: 0.1 }, coord! { x: 0.9, y: 0.1 }]);
        let tiles = render(&layer, FeatureGeom::Lines(std::slice::from_ref(&line)));
        assert_eq!(tiles.len(), 2);
        for (_, _, geom) in &tiles {
            assert_eq!(geom.vertices.len(), 2);
        }
        assert_eq!(tiles[1].2.vertices, [[51 - 256, 51], [461 - 256, 51]]);
    }

    fn square(lo: f64, hi: f64) -> LineString<f64> {
        LineString::from(vec![(lo, lo), (hi, lo), (hi, hi), (lo, hi), (lo, lo)])
    }

    #[test]
    fn polygon_covers_its_tiles_with_fill_ranges() {
        for order in [TileOrder::Tms, TileOrder::Hilbert] {
            let layer = RenderLayer::new(0, 5..=5, GRID).unwrap();
            let polygon = Polygon::new(square(0.1, 0.9), vec![]);
            let (tiles, records) = polygon_tiles(order, &layer, std::slice::from_ref(&polygon));
            // A tile is reached if its buffered box overlaps the square.
            let margin = f64::from(GRID.buffer) / f64::from(GRID.extent);
            let reached = |t: u32| {
                (f64::from(t) - margin) / 32.0 < 0.9 && (f64::from(t + 1) + margin) / 32.0 > 0.1
            };
            let expected: Vec<_> = (0..32)
                .flat_map(|x| (0..32).map(move |y| (5, x, y)))
                .filter(|&(_, x, y)| reached(x) && reached(y))
                .collect();
            assert_eq!(tiles, expected, "{order:?}");
            assert!(
                records < tiles.len() / 4,
                "{order:?}: {records} records for {} tiles",
                tiles.len()
            );
        }
    }

    #[test]
    fn polygons_get_mvt_winding_and_keep_holes_empty() {
        let layer = RenderLayer::new(0, 4..=4, GRID).unwrap();
        // Clockwise exterior in unit coordinates (negative area, y down), with a large hole.
        let mut exterior = square(0.05, 0.95);
        exterior.0.reverse();
        let polygon = Polygon::new(exterior, vec![square(0.3, 0.7)]);
        let records = render_in(
            TileOrder::Tms,
            &layer,
            FeatureGeom::Polygons(std::slice::from_ref(&polygon)),
        );
        for (_, kind, geom) in records.iter().filter(|r| r.1 == GeomKind::Polygon) {
            let mut rest = geom.vertices.as_slice();
            let mut rings = geom.parts.iter();
            for &count in &geom.polygons {
                for i in 0..count {
                    let (ring, tail) = rest.split_at(*rings.next().unwrap() as usize);
                    rest = tail;
                    let coords: Vec<_> = ring.iter().map(|&[x, y]| Coord { x, y }).collect();
                    let area = signed_area_2x(&coords);
                    assert!(
                        if i == 0 { area > 0 } else { area < 0 },
                        "{kind:?} ring {i} area {area}"
                    );
                }
            }
        }
        let (tiles, _) = polygon_tiles(TileOrder::Tms, &layer, std::slice::from_ref(&polygon));
        // Tiles 6..10 lie wholly inside the hole (0.3..0.7 of 16 tiles is 4.8..11.2).
        assert!(!tiles.contains(&(4, 7, 7)) && !tiles.contains(&(4, 8, 9)));
        assert!(tiles.contains(&(4, 2, 2)), "covered by the exterior");
    }

    #[test]
    fn bounds_drop_tiles_outside() {
        let mut layer = RenderLayer::new(0, 1..=1, GRID).unwrap();
        layer.min_size = (0.0, 0.0);
        layer.bounds = Some([0.0, 0.0, 0.4, 0.4]);
        let line = LineString::from(vec![coord! { x: 0.1, y: 0.1 }, coord! { x: 0.9, y: 0.1 }]);
        let tiles = render(&layer, FeatureGeom::Lines(std::slice::from_ref(&line)));
        assert_eq!(
            tiles.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
            ["1/0/0"]
        );
    }

    #[test]
    fn rejects_zooms_beyond_the_grid() {
        RenderLayer::new(
            0,
            0..=18,
            LayerGrid {
                extent: 4096,
                buffer: 64,
            },
        )
        .unwrap();
        RenderLayer::new(
            0,
            0..=19,
            LayerGrid {
                extent: 4096,
                buffer: 64,
            },
        )
        .unwrap_err();
        RenderLayer::new(
            0,
            0..=5,
            LayerGrid {
                extent: 256,
                buffer: 128,
            },
        )
        .unwrap_err();
    }

    /// Records a renderer call leaves, as `(tile id, bytes)`.
    fn records(fill: impl FnOnce(&mut Renderer, &mut SortBuffer<'_>)) -> Vec<(u64, Vec<u8>)> {
        let dir = tempfile::tempdir().unwrap();
        let sorter = Sorter::new(SortConfig {
            temp_dirs: vec![dir.path().to_path_buf()],
            buffer_bytes: 1 << 20,
            max_fan_in: 8,
            read_buffer_bytes: 4096,
        })
        .unwrap();
        let mut buffer = sorter.buffer();
        fill(&mut Renderer::default(), &mut buffer);
        buffer.finish().unwrap();
        let mut merger = sorter.merge().unwrap();
        let mut out = Vec::new();
        while let Some((key, bytes)) = merger.next().unwrap() {
            out.push((key.tile_id(), bytes.to_vec()));
        }
        out
    }

    #[test]
    #[expect(clippy::cast_possible_truncation, reason = "the hull of i32 vertices")]
    fn sole_tile_features_match_the_slicer() {
        let layer = RenderLayer::new(0, 4..=4, GRID).unwrap();
        let ctx = Ctx {
            order: TileOrder::Tms,
            layer: &layer,
            seq: Seq::default(),
            id: Some(7),
        };
        let mut state = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = move |n: i32| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            i32::try_from(state % u64::from(n.unsigned_abs())).unwrap()
        };
        let (mut sole, mut sliced) = (0, 0);
        for case in 0..3000 {
            let (cx, cy, r) = (next(16 * 256), next(16 * 256), 1 + next(40));
            let mut ring: Vec<_> = (0..3 + next(6))
                .map(|_| Coord {
                    x: cx + next(2 * r) - r,
                    y: cy + next(2 * r) - r,
                })
                .collect();
            ring.dedup();
            if case % 2 == 0 {
                let parts = [len32(ring.len()).unwrap()];
                if ring.len() < 2 || sole_tile(&ring, GRID).unwrap().is_none() {
                    continue;
                }
                let fast = records(|r, out| {
                    let tile = sole_tile(&ring, GRID).unwrap().unwrap();
                    r.push_whole(&ctx, 4, tile, (&[], &parts, &ring), out)
                        .unwrap();
                });
                let slow = records(|r, out| r.slice_lines(&ctx, 4, &parts, &ring, out).unwrap());
                assert_eq!(fast, slow, "line {ring:?}");
            } else {
                // A convex hull, wound as the renderer winds exteriors.
                let mut hull = geo::ConvexHull::convex_hull(&geo_types::MultiPoint::from(
                    ring.iter()
                        .map(|c| (f64::from(c.x), f64::from(c.y)))
                        .collect::<Vec<_>>(),
                ))
                .exterior()
                .0
                .iter()
                .map(|c| Coord {
                    x: c.x as i32,
                    y: c.y as i32,
                })
                .collect::<Vec<_>>();
                hull.pop();
                if hull.len() < 3 || signed_area_2x(&hull) == 0 {
                    continue;
                }
                if signed_area_2x(&hull) < 0 {
                    hull.reverse();
                }
                let (polys, parts) = ([1], [len32(hull.len()).unwrap()]);
                let Some(tile) = sole_tile(&hull, GRID).unwrap() else {
                    sliced += 1;
                    continue;
                };
                let fast = records(|r, out| {
                    r.push_whole(&ctx, 4, tile, (&polys, &parts, &hull), out)
                        .unwrap();
                });
                let slow = records(|r, out| {
                    r.slice_polygons(&ctx, 4, &polys, &parts, &hull, out)
                        .unwrap();
                });
                assert_eq!(fast, slow, "polygon {hull:?}");
            }
            sole += 1;
        }
        assert!(sole > 1000 && sliced > 50, "{sole} sole, {sliced} sliced");
    }
}
