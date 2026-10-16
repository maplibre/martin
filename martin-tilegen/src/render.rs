//! Renders one feature into tile pieces for every zoom it has in its layer, pushed straight into a sort buffer.

use std::ops::{Range, RangeInclusive};

use geo::BoundingRect;
use geo_types::{Coord, LineString, Polygon, Rect};
use map_tile_toolkit::{ClipMode, PolygonSlice, Slicer, signed_area_2x};
use martin_tile_utils::{TileCoord, TileRect, bbox_to_xyz};
use tilejson::Bounds;

use crate::record::{EncodedProps, Geom, Vertex, encode};
use crate::simplify::Simplifier;
use crate::{
    LayerGrid, LayerId, Seq, SortBuffer, SortKey, TileGenError, TileGenResult, TileId, TileOrder,
};

/// Simplification and size thresholds are in pixels of a 256-pixel tile, as in Planetiler.
const TILE_PIXELS: f64 = 256.0;

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum RenderLayerError {
    #[error("the zoom range is empty")]
    EmptyZooms,

    #[error("extent {extent} at max zoom {max_zoom} overflows the i32 grid")]
    ZoomGridOverflow { extent: u32, max_zoom: u8 },

    #[error("buffer {buffer} is not below half the extent {extent} or above 65535")]
    InvalidBuffer { buffer: u32, extent: u32 },
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum GeometryError {
    #[error("a coordinate is outside the i32 zoom grid")]
    CoordOverflow,

    #[error("a line with {0} vertices does not fit a record")]
    TooManyVertices(usize),

    #[error(transparent)]
    Slice(#[from] map_tile_toolkit::TileError),
}

/// A feature in Web Mercator unit coordinates (see [`project`](crate::project)), as one layer renders it.
pub struct Feature<'a> {
    pub id: Option<u64>,
    pub geom: FeatureGeom<'a>,
    pub props: &'a EncodedProps,
    /// From each zoom on, ascending, these replace `props`.
    pub bands: &'a [(u8, EncodedProps)],
    /// The `sort_by` key of the feature in its layer, if the layer has one.
    pub sort: Option<&'a [u8]>,
    /// Rendered only where these meet the layer's zooms.
    pub zooms: RangeInclusive<u8>,
    /// RDP tolerance.
    pub simplify: PixelThreshold,
    /// Lines and polygons whose bounding box is smaller than this are dropped. Size only shrinks with
    /// zoom, so a dropped feature stays dropped.
    pub min_size: PixelThreshold,
}

#[derive(Clone, Copy)]
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
    pub clip_mode: ClipMode,
    /// Per zoom from `zooms.start()`, the only tiles kept.
    tiles: Option<Vec<TileRect>>,
}

/// Pixels of a 256-pixel tile, with a separate value for the layer's max zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PixelThreshold {
    pub below_max_zoom: f64,
    pub at_max_zoom: f64,
}

impl PixelThreshold {
    pub const ZERO: Self = Self {
        below_max_zoom: 0.0,
        at_max_zoom: 0.0,
    };

    /// 0.1 px, and 1/16 px at the max zoom, matching Planetiler.
    pub const DEFAULT_SIMPLIFY: Self = Self {
        below_max_zoom: 0.1,
        at_max_zoom: 0.0625,
    };

    /// 1 px, and 1/16 px at the max zoom, matching Planetiler.
    pub const DEFAULT_MIN_SIZE: Self = Self {
        below_max_zoom: 1.0,
        at_max_zoom: 0.0625,
    };
}

impl RenderLayer {
    /// A clipped layer keeping every tile in `zooms`.
    pub fn new(
        index: u8,
        zooms: RangeInclusive<u8>,
        grid: LayerGrid,
    ) -> Result<Self, RenderLayerError> {
        let max_zoom = *zooms.end();
        if zooms.is_empty() {
            return Err(RenderLayerError::EmptyZooms);
        }
        let fits = u64::from(grid.extent)
            .checked_shl(u32::from(max_zoom))
            .is_some_and(|s| s <= 1 << 30);
        if !fits {
            return Err(RenderLayerError::ZoomGridOverflow {
                extent: grid.extent,
                max_zoom,
            });
        }
        if grid.buffer >= grid.extent / 2 || u16::try_from(grid.buffer).is_err() {
            return Err(invalid_buffer(grid));
        }
        Ok(Self {
            index,
            zooms,
            grid,
            clip_mode: ClipMode::Clipped,
            tiles: None,
        })
    }

    /// [`ClipMode::Unclipped`] gives every tile each line or polygon of a feature that reaches it,
    /// whole, so polygons get no fill ranges.
    #[must_use]
    pub fn with_clip_mode(mut self, clip_mode: ClipMode) -> Self {
        self.clip_mode = clip_mode;
        self
    }

    /// Keeps only the tiles intersecting the WGS84 `bounds`.
    #[must_use]
    pub fn with_bounds(mut self, bounds: Bounds) -> Self {
        let tiles = self.zooms.clone().map(|zoom| {
            let (min_x, min_y, max_x, max_y) =
                bbox_to_xyz(bounds.left, bounds.bottom, bounds.right, bounds.top, zoom);
            TileRect::new(zoom, min_x, min_y, max_x, max_y)
        });
        self.tiles = Some(tiles.collect());
        self
    }

    /// The zooms both the layer and the feature cover.
    fn zooms_for(&self, feature: &Feature<'_>) -> RangeInclusive<u8> {
        let start = (*self.zooms.start()).max(*feature.zooms.start());
        let end = (*self.zooms.end()).min(*feature.zooms.end());
        start..=end
    }

    fn contains(&self, zoom: u8, x: u32, y: u32) -> bool {
        let (xs, ys) = self.tile_bounds(zoom);
        xs.contains(&x) && ys.contains(&y)
    }

    /// The tile columns and rows of `zoom` that [`with_bounds`](Self::with_bounds) allows.
    fn tile_bounds(&self, zoom: u8) -> (Range<u32>, Range<u32>) {
        let Some(tiles) = &self.tiles else {
            let all = 0..1 << zoom;
            return (all.clone(), all);
        };
        let rect = tiles[usize::from(zoom - self.zooms.start())];
        (rect.min_x..rect.max_x + 1, rect.min_y..rect.max_y + 1)
    }

    /// The RDP tolerance at `zoom` in Web Mercator units.
    fn epsilon(&self, zoom: u8, simplify: PixelThreshold) -> f64 {
        self.pixels(zoom, simplify) / (TILE_PIXELS * scale(zoom))
    }

    /// The smallest tolerance over `zooms` that simplifies at all, or infinity.
    fn min_epsilon(&self, zooms: RangeInclusive<u8>, simplify: PixelThreshold) -> f64 {
        zooms
            .map(|zoom| self.epsilon(zoom, simplify))
            .filter(|&epsilon| epsilon > 0.0)
            .reduce(f64::min)
            .unwrap_or(f64::INFINITY)
    }

    /// Picks the threshold value that applies at `zoom`.
    fn pixels(&self, zoom: u8, threshold: PixelThreshold) -> f64 {
        if zoom == *self.zooms.end() {
            threshold.at_max_zoom
        } else {
            threshold.below_max_zoom
        }
    }
}

#[derive(Default)]
pub struct Renderer {
    /// One slicer per layer grid and clip mode, for lines and polygons, reused across features.
    slicers: Vec<((LayerGrid, ClipMode), Slicer)>,
    /// The quantized feature in the zoom's global grid: ring counts per polygon, line or ring lengths,
    /// and vertices.
    polys: Vec<u32>,
    parts: Vec<u32>,
    vertices: Vec<Coord<i32>>,
    simplifier: Simplifier,
    /// One tile's piece, shaped the same with tile-local vertices.
    piece: PieceBuf,
    /// The slicer's output for the feature at one zoom, which comes in no particular order: every
    /// line run or polygon ring with its tile, its vertices tile-local in `sliced_vertices`.
    sliced: Vec<SlicedPart>,
    sliced_vertices: Vec<Vertex>,
    points: Vec<(i32, i32, Vertex)>,
    /// Fill runs as `(x_start, x_end, y)`, and the tile-id ranges they become.
    runs: Vec<(i32, i32, i32)>,
    ranges: Vec<Range<u64>>,
    /// Features skipped at a zoom because they could not be quantized or sliced, by layer index.
    pub slice_errors: Vec<SliceErrors>,
}

#[derive(Debug, Default)]
pub struct SliceErrors {
    pub count: u64,
    /// The failure with the lowest [`Seq`], so the reported feature does not depend on threading.
    pub first: Option<SliceError>,
}

#[derive(Debug)]
pub struct SliceError {
    pub seq: Seq,
    pub id: Option<u64>,
    pub zoom: u8,
    pub error: GeometryError,
}

impl SliceErrors {
    pub fn merge(&mut self, other: Self) {
        self.count += other.count;
        if let Some(error) = other.first {
            self.record_first(error);
        }
    }

    fn record_first(&mut self, error: SliceError) {
        if self
            .first
            .as_ref()
            .is_none_or(|first| error.seq < first.seq)
        {
            self.first = Some(error);
        }
    }
}

impl Renderer {
    /// Pushes the feature's pieces for every zoom in [`RenderLayer::zooms_for`], highest zoom first.
    pub fn render(
        &mut self,
        order: TileOrder,
        layer: &RenderLayer,
        seq: Seq,
        feature: &Feature<'_>,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let ctx = Ctx {
            order,
            layer,
            seq,
            feature,
        };
        let zooms = layer.zooms_for(feature);
        match feature.geom {
            FeatureGeom::Points(points) => {
                let Some(bbox) = points.iter().map(BoundingRect::bounding_rect).reduce(union)
                else {
                    return Ok(());
                };
                let shift = world_shift(bbox);
                for zoom in zooms.rev() {
                    let result = self.render_points(&ctx, zoom, points, shift, out);
                    self.skip_unrenderable(&ctx, zoom, result)?;
                }
            }
            FeatureGeom::Lines(lines) => {
                let bbox = lines
                    .iter()
                    .filter_map(BoundingRect::bounding_rect)
                    .reduce(union);
                let min_epsilon = layer.min_epsilon(zooms.clone(), feature.simplify);
                self.simplifier.set_lines(lines, min_epsilon);
                self.render_zooms(&ctx, zooms, bbox, out, |r, zoom, shift, out| {
                    r.render_lines(&ctx, zoom, shift, out)
                })?;
            }
            FeatureGeom::Polygons(polygons) => {
                let bbox = polygons
                    .iter()
                    .filter_map(BoundingRect::bounding_rect)
                    .reduce(union);
                let min_epsilon = layer.min_epsilon(zooms.clone(), feature.simplify);
                self.simplifier.set_polygons(polygons, min_epsilon);
                self.render_zooms(&ctx, zooms, bbox, out, |r, zoom, shift, out| {
                    r.render_polygons(&ctx, zoom, shift, out)
                })?;
            }
        }
        Ok(())
    }

    /// Max zoom first, stopping once the feature is below the minimum size (size only shrinks with zoom).
    /// A zoom the slicer cannot handle is counted and skipped; lower zooms span fewer tiles and may work.
    fn render_zooms(
        &mut self,
        ctx: &Ctx<'_>,
        zooms: RangeInclusive<u8>,
        bbox: Option<Rect<f64>>,
        out: &mut SortBuffer<'_>,
        mut render: impl FnMut(&mut Self, u8, f64, &mut SortBuffer<'_>) -> TileGenResult<()>,
    ) -> TileGenResult<()> {
        let Some(bbox) = bbox else {
            return Ok(());
        };
        let shift = world_shift(bbox);
        for zoom in zooms.rev() {
            let size = bbox.width().max(bbox.height()) * TILE_PIXELS * scale(zoom);
            if size < ctx.layer.pixels(zoom, ctx.feature.min_size) {
                break;
            }
            let result = render(self, zoom, shift, out);
            self.skip_unrenderable(ctx, zoom, result)?;
        }
        Ok(())
    }

    /// Records a feature that could not be quantized or sliced at `zoom` and lets rendering go on;
    /// any other error is returned.
    fn skip_unrenderable(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        result: TileGenResult<()>,
    ) -> TileGenResult<()> {
        match result {
            Err(TileGenError::Geometry(
                error @ (GeometryError::Slice(_) | GeometryError::CoordOverflow),
            )) => {
                let index = usize::from(ctx.layer.index);
                if self.slice_errors.len() <= index {
                    self.slice_errors
                        .resize_with(index + 1, SliceErrors::default);
                }
                let errors = &mut self.slice_errors[index];
                errors.count += 1;
                errors.record_first(SliceError {
                    seq: ctx.seq,
                    id: ctx.feature.id,
                    zoom,
                    error,
                });
                Ok(())
            }
            other => other,
        }
    }

    /// Quantizes the points at `zoom` and pushes one piece per tile, including neighbors whose buffer
    /// a point reaches.
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
        for group in self.points.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)) {
            let (tx, ty, _) = group[0];
            self.piece.clear();
            self.piece.vertices.extend(group.iter().map(|p| p.2));
            ctx.push(zoom, tx, ty, Geom::Points(&self.piece.vertices), out)?;
        }
        Ok(())
    }

    /// Simplifies and quantizes the lines at `zoom`, slices them into tiles and pushes one piece per
    /// tile. Called once per feature and zoom.
    fn render_lines(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        shift: f64,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.parts.clear();
        self.vertices.clear();
        let epsilon = ctx.layer.epsilon(zoom, ctx.feature.simplify);
        let world = scale(zoom) * f64::from(ctx.layer.grid.extent);
        for line in self.simplifier.lines(epsilon) {
            let start = self.vertices.len();
            match push_quantized(&mut self.vertices, line, shift, world)? {
                0 | 1 => self.vertices.truncate(start),
                n => self.parts.push(vertex_count(n)?),
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

    /// Slices each line into the tiles it reaches and pushes one piece per tile, holding every run
    /// of the feature's lines there (unclipped, every line that reaches the tile, whole). A slicer error drops the whole feature at this zoom: nothing is
    /// pushed until every line is sliced.
    fn slice_lines(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        parts: &[u32],
        vertices: &[Coord<i32>],
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let pos = self.slicer_index(ctx.layer)?;
        let slicer = &self.slicers[pos].1;
        self.sliced.clear();
        self.sliced_vertices.clear();
        let mut rest = vertices;
        for &len in parts {
            let (line, tail) = rest.split_at(len as usize);
            rest = tail;
            for slice in slicer.line(line).map_err(GeometryError::from)? {
                let start = self.sliced_vertices.len();
                self.sliced_vertices
                    .extend(slice.run().iter().map(|c| [c.x, c.y]));
                self.sliced.push(SlicedPart {
                    tile: slice.tile(),
                    polygon: 0,
                    start,
                    len: vertex_count(self.sliced_vertices.len() - start)?,
                });
            }
        }
        self.push_sliced(ctx, zoom, PieceKind::Lines, out)
    }

    /// Pushes one piece per tile from [`Self::sliced`]: the tile's parts in the order they were
    /// sliced (a stable sort by tile keeps each line's runs, and each polygon's rings, together and
    /// in order).
    fn push_sliced(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        kind: PieceKind,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.sliced.sort_by_key(|p| (p.tile.x, p.tile.y));
        for tile in self.sliced.chunk_by(|a, b| a.tile == b.tile) {
            self.piece.clear();
            for polygon in tile.chunk_by(|a, b| a.polygon == b.polygon) {
                for part in polygon {
                    self.piece.parts.push(part.len);
                    self.piece.vertices.extend_from_slice(
                        &self.sliced_vertices[part.start..part.start + part.len as usize],
                    );
                }
                if kind == PieceKind::Polygons {
                    self.piece.polys.push(len32(polygon.len())?);
                }
            }
            let geom = match kind {
                PieceKind::Lines => self.piece.lines(),
                PieceKind::Polygons => self.piece.polygons(),
            };
            ctx.push(zoom, tile[0].tile.x, tile[0].tile.y, geom, out)?;
        }
        Ok(())
    }

    fn render_polygons(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        shift: f64,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.polys.clear();
        self.parts.clear();
        self.vertices.clear();
        let epsilon = ctx.layer.epsilon(zoom, ctx.feature.simplify);
        let world = scale(zoom) * f64::from(ctx.layer.grid.extent);
        for rings in self.simplifier.polygons(epsilon) {
            let first_ring = self.parts.len();
            for (i, ring) in rings.enumerate() {
                let start = self.vertices.len();
                let mut n = push_quantized(&mut self.vertices, ring, shift, world)?;
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
                self.parts.push(vertex_count(n)?);
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
        ctx.push(zoom, tx, ty, geom, out)
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
        let mut rings = Vec::with_capacity(parts.len());
        let mut rest = vertices;
        for &len in parts {
            let (ring, tail) = rest.split_at(len as usize);
            rings.push(ring);
            rest = tail;
        }
        let pos = self.slicer_index(ctx.layer)?;
        let slicer = &mut self.slicers[pos].1;
        self.sliced.clear();
        self.sliced_vertices.clear();
        self.runs.clear();
        // Each polygon is sliced on its own; its pieces and those of the feature's other polygons in
        // a tile make one piece there, and nothing is pushed until every polygon is sliced.
        let mut rest = rings.as_slice();
        for (polygon, &count) in (0_u32..).zip(polys) {
            let (polygon_rings, tail) = rest.split_at(count as usize);
            rest = tail;
            let mut pieces = slicer.polygon(polygon_rings).map_err(GeometryError::from)?;
            while let Some(slice) = pieces.next() {
                match slice.map_err(GeometryError::from)? {
                    PolygonSlice::Piece(piece) => {
                        for ring in piece.rings() {
                            let start = self.sliced_vertices.len();
                            self.sliced_vertices.extend(ring.iter().map(|c| [c.x, c.y]));
                            // The slicer closes rings; records store them open.
                            self.sliced_vertices.pop();
                            self.sliced.push(SlicedPart {
                                tile: piece.tile(),
                                polygon,
                                start,
                                len: vertex_count(self.sliced_vertices.len() - start)?,
                            });
                        }
                    }
                    // Only clipped: an unclipped slicer gives covered tiles the whole polygon.
                    PolygonSlice::Run(run) => self.runs.push((run.x.start, run.x.end, run.y)),
                }
            }
        }
        self.push_sliced(ctx, zoom, PieceKind::Polygons, out)?;
        self.push_fills(ctx, zoom, out)
    }

    /// Rows with the same column span merge into rectangles, which become contiguous id ranges.
    fn push_fills(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.runs.sort_unstable();
        let side = 1i32 << zoom;
        let (allowed_x, allowed_y) = ctx.layer.tile_bounds(zoom);
        let runs = std::mem::take(&mut self.runs);
        let mut result = Ok(());
        for group in
            runs.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1) && a.2.checked_add(1) == Some(b.2))
        {
            if result.is_err() {
                break;
            }
            let (x0, x1, y0) = group[0];
            let rows = group.len();
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
                    result = self.push_rect(ctx, zoom, (xs, ys.clone()), out);
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
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        self.ranges.clear();
        ctx.order.fill_ranges(zoom, xs, ys, &mut self.ranges)?;
        for range in &self.ranges {
            ctx.push_id(
                zoom,
                TileId::new(range.start)?,
                Geom::FillRange { end: range.end },
                out,
            )?;
        }
        Ok(())
    }

    /// The slicer for the layer's grid and clip mode, created on first use and reused across features.
    fn slicer_index(&mut self, layer: &RenderLayer) -> TileGenResult<usize> {
        let key = (layer.grid, layer.clip_mode);
        if let Some(pos) = self.slicers.iter().position(|(k, _)| *k == key) {
            return Ok(pos);
        }
        let grid = layer.grid;
        let buffer = u16::try_from(grid.buffer).map_err(|_too_large| invalid_buffer(grid))?;
        let slicer = Slicer::new(grid.extent, buffer)
            .map_err(GeometryError::from)?
            .with_clip_mode(layer.clip_mode);
        self.slicers.push((key, slicer));
        Ok(self.slicers.len() - 1)
    }
}

/// What every piece of one [`Renderer::render`] call shares: the sort order, the layer, and the
/// feature with its [`Seq`].
struct Ctx<'a> {
    order: TileOrder,
    layer: &'a RenderLayer,
    seq: Seq,
    feature: &'a Feature<'a>,
}

impl Ctx<'_> {
    /// Tiles past the antimeridian wrap around; tiles past the poles (only ever buffer) are dropped.
    fn push(
        &self,
        zoom: u8,
        tx: i32,
        ty: i32,
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
            zoom,
            self.order.tile_id(TileCoord::new_unchecked(zoom, x, y))?,
            geom,
            out,
        )
    }

    fn push_id(
        &self,
        zoom: u8,
        tile_id: TileId,
        geom: Geom<'_>,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let props = self.props_at(zoom);
        out.push_with(
            SortKey::new(tile_id, LayerId::new(self.layer.index), self.seq),
            |buf| {
                encode(buf, self.feature.id, self.feature.sort, props, geom);
            },
        )
    }

    fn props_at(&self, zoom: u8) -> &EncodedProps {
        self.feature
            .bands
            .iter()
            .rev()
            .find(|(from, _)| *from <= zoom)
            .map_or(self.feature.props, |(_, props)| props)
    }
}

/// A quantized feature in its zoom's global grid: ring counts per polygon, line or ring lengths, vertices.
type Rings<'a> = (&'a [u32], &'a [u32], &'a [Coord<i32>]);

/// One line run or polygon ring the slicer put in `tile`, its vertices at `start..start + len` of
/// [`Renderer::sliced_vertices`]. `polygon` numbers the feature's polygons, so a polygon's rings stay
/// together in a tile.
#[derive(Clone, Copy)]
struct SlicedPart {
    tile: map_tile_toolkit::TileId,
    polygon: u32,
    start: usize,
    len: u32,
}

/// The shape of the pieces [`Renderer::push_sliced`] builds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum PieceKind {
    Lines,
    Polygons,
}

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

    /// The whole feature in the frame of the tile at `origin`.
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

fn vertex_count(n: usize) -> Result<u32, GeometryError> {
    u32::try_from(n).map_err(|_overflow| GeometryError::TooManyVertices(n))
}

/// Tiles per world side at `zoom`.
fn scale(zoom: u8) -> f64 {
    f64::from(1u32 << zoom)
}

fn to_i32(v: u32) -> Result<i32, GeometryError> {
    i32::try_from(v).map_err(|_overflow| GeometryError::CoordOverflow)
}

fn invalid_buffer(grid: LayerGrid) -> RenderLayerError {
    RenderLayerError::InvalidBuffer {
        buffer: grid.buffer,
        extent: grid.extent,
    }
}

/// Quantizes and appends `line` without consecutive duplicates; returns how many vertices it kept.
fn push_quantized(
    vertices: &mut Vec<Coord<i32>>,
    line: impl Iterator<Item = Coord<f64>>,
    shift: f64,
    world: f64,
) -> TileGenResult<usize> {
    let start = vertices.len();
    for c in line {
        let [x, y] = quantize(c, shift, world)?;
        let c = Coord { x, y };
        if vertices.len() == start || vertices.last() != Some(&c) {
            vertices.push(c);
        }
    }
    Ok(vertices.len() - start)
}

/// Rounds to the zoom grid; `shift` moves features lying wholly outside the world back into it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "range-checked before the cast"
)]
fn quantize(c: Coord<f64>, shift: f64, world: f64) -> Result<Vertex, GeometryError> {
    let (x, y) = (((c.x + shift) * world).round(), (c.y * world).round());
    let range = f64::from(i32::MIN)..=f64::from(i32::MAX);
    if range.contains(&x) && range.contains(&y) {
        Ok([x as i32, y as i32])
    } else {
        Err(GeometryError::CoordOverflow)
    }
}

/// Whole world widths that bring a feature lying entirely outside `[0, 1)` back into it, e.g. data drawn
/// at longitudes 181..190 to avoid splitting Fiji. A feature straddling the edge stays and is wrapped
/// per tile instead.
fn world_shift(bbox: Rect<f64>) -> f64 {
    let (lo, hi) = (bbox.min().x, bbox.max().x);
    if lo >= 1.0 || hi < 0.0 {
        -lo.floor()
    } else {
        0.0
    }
}

fn union(a: Rect<f64>, b: Rect<f64>) -> Rect<f64> {
    Rect::new(
        Coord {
            x: a.min().x.min(b.min().x),
            y: a.min().y.min(b.min().y),
        },
        Coord {
            x: a.max().x.max(b.max().x),
            y: a.max().y.max(b.max().y),
        },
    )
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use geo::SimplifyIdx as _;
    use geo_types::coord;

    use super::*;
    use crate::project::from_lonlat;
    use crate::record::{GeomBuf, GeomKind, Record};
    use crate::{SortConfig, Sorter};

    const GRID: LayerGrid = LayerGrid {
        extent: 256,
        buffer: 8,
    };

    fn render(layer: &RenderLayer, geom: FeatureGeom<'_>) -> Vec<(String, GeomKind, GeomBuf)> {
        render_feature(
            layer,
            &Feature {
                id: Some(1),
                geom,
                props: &EncodedProps::default(),
                bands: &[],
                sort: None,
                zooms: layer.zooms.clone(),
                simplify: PixelThreshold::DEFAULT_SIMPLIFY,
                min_size: PixelThreshold::DEFAULT_MIN_SIZE,
            },
        )
    }

    fn render_feature(
        layer: &RenderLayer,
        feature: &Feature<'_>,
    ) -> Vec<(String, GeomKind, GeomBuf)> {
        let mut out: Vec<_> = render_in(TileOrder::Tms, layer, feature)
            .into_iter()
            .map(|(id, kind, geom)| {
                (
                    format!(
                        "{:#}",
                        TileOrder::Tms.tile_coord(TileId::new(id).unwrap()).unwrap()
                    ),
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
        feature: &Feature<'_>,
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
        Renderer::default()
            .render(order, layer, Seq::default(), feature, &mut buffer)
            .unwrap();
        buffer.finish().unwrap();
        let mut merger = sorter.merge().unwrap();
        let mut out = Vec::new();
        while let Some((key, bytes)) = merger.next_record().unwrap() {
            let record = Record::decode(bytes).unwrap();
            let mut geom = GeomBuf::default();
            record.append_geometry(&mut geom).unwrap();
            out.push((key.tile_id().value(), record.kind, geom));
        }
        out
    }

    /// Every tile a polygon reaches: edge pieces, plus the tiles of each fill range.
    fn polygon_tiles(
        order: TileOrder,
        layer: &RenderLayer,
        polygons: &[Polygon<f64>],
    ) -> (Vec<(u8, u32, u32)>, usize) {
        let feature = Feature {
            id: Some(1),
            geom: FeatureGeom::Polygons(polygons),
            props: &EncodedProps::default(),
            bands: &[],
            sort: None,
            zooms: layer.zooms.clone(),
            simplify: PixelThreshold::DEFAULT_SIMPLIFY,
            min_size: PixelThreshold::DEFAULT_MIN_SIZE,
        };
        let records = render_in(order, layer, &feature);
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
                let c = order.tile_coord(TileId::new(id).unwrap()).unwrap();
                tiles.push((c.z(), c.x(), c.y()));
            }
        }
        tiles.sort_unstable();
        (tiles, records.len())
    }

    #[test]
    fn point_near_an_edge_reaches_the_neighbor_buffer() {
        let layer = RenderLayer::new(0, 1..=1, GRID).unwrap();
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
        let line: LineString = [coord! { x: -0.5, y: 10.0 }, coord! { x: 0.5, y: 10.0 }]
            .into_iter()
            .map(from_lonlat)
            .collect();
        let tiles = render(&layer, FeatureGeom::Lines(std::slice::from_ref(&line)));
        let zooms: Vec<_> = tiles
            .iter()
            .map(|t| t.0.split('/').next().unwrap())
            .collect();
        assert_eq!(zooms, ["1", "1", "2", "2", "3", "3"]);
        assert!(tiles.iter().all(|t| t.1 == GeomKind::Line));
    }

    #[test]
    fn simplification_drops_vertices_at_low_zoom() {
        let layer = RenderLayer::new(0, 0..=8, GRID).unwrap();
        let zigzag: Vec<_> = (0..=100)
            .map(|i| coord! { x: f64::from(i) * 0.01, y: f64::from(i % 2) * 0.001 })
            .map(from_lonlat)
            .collect();
        let tiles = render_feature(
            &layer,
            &Feature {
                id: Some(1),
                geom: FeatureGeom::Lines(&[LineString::from(zigzag)]),
                props: &EncodedProps::default(),
                bands: &[],
                sort: None,
                zooms: 0..=8,
                simplify: PixelThreshold::DEFAULT_SIMPLIFY,
                min_size: PixelThreshold::ZERO,
            },
        );
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
    fn every_zoom_simplifies_as_douglas_peucker_on_the_full_geometry() {
        let layer = RenderLayer::new(
            0,
            0..=14,
            LayerGrid {
                extent: 4096,
                buffer: 64,
            },
        )
        .unwrap();
        let mut state = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            #[expect(clippy::cast_precision_loss, reason = "53 random bits")]
            let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
            unit * 2.0 - 1.0
        };
        let mut lines: Vec<LineString> = Vec::new();
        for step in [1e-2, 1e-3, 1e-4, 1e-5].repeat(5) {
            let mut at = coord! { x: 13.4 + next() * 0.1, y: 52.5 + next() * 0.1 };
            lines.push(
                (0..400)
                    .map(|_| {
                        at = coord! { x: at.x + next() * step, y: at.y + next() * step };
                        from_lonlat(at)
                    })
                    .collect(),
            );
        }
        for radius in [1e-1, 1e-2, 1e-3, 1e-4] {
            let center = coord! { x: 13.4 + next() * 0.1, y: 52.5 + next() * 0.1 };
            let mut ring: LineString = (0..300)
                .map(|i| {
                    let angle = TAU * f64::from(i) / 300.0;
                    let r = radius * (1.0 + 0.2 * next());
                    from_lonlat(
                        coord! { x: center.x + r * angle.cos(), y: center.y + r * angle.sin() },
                    )
                })
                .collect();
            ring.close();
            lines.push(ring);
        }
        let props = EncodedProps::default();

        for simplify in [
            PixelThreshold::DEFAULT_SIMPLIFY,
            PixelThreshold {
                below_max_zoom: 0.5,
                at_max_zoom: 4.0,
            },
        ] {
            let all_zooms = Feature {
                id: Some(1),
                geom: FeatureGeom::Lines(&lines),
                props: &props,
                bands: &[],
                sort: None,
                zooms: 0..=14,
                simplify,
                min_size: PixelThreshold::ZERO,
            };
            let mut per_zoom = Vec::new();
            for zoom in 0..=14 {
                let pixels = if zoom == 14 {
                    simplify.at_max_zoom
                } else {
                    simplify.below_max_zoom
                };
                let epsilon = pixels / (256.0 * f64::from(1u32 << zoom));
                let simplified: Vec<LineString> = lines
                    .iter()
                    .map(|line| {
                        line.simplify_idx(epsilon)
                            .into_iter()
                            .map(|i| line.0[i])
                            .collect()
                    })
                    .collect();
                per_zoom.extend(render_feature(
                    &layer,
                    &Feature {
                        geom: FeatureGeom::Lines(&simplified),
                        zooms: zoom..=zoom,
                        simplify: PixelThreshold::ZERO,
                        ..all_zooms
                    },
                ));
            }
            per_zoom.sort_by(|a, b| a.0.cmp(&b.0));

            assert_eq!(render_feature(&layer, &all_zooms), per_zoom, "{simplify:?}");
        }
    }

    #[test]
    fn features_past_the_antimeridian_move_into_the_world() {
        let layer = RenderLayer::new(0, 2..=2, GRID).unwrap();
        let tiles = render(
            &layer,
            FeatureGeom::Points(&[coord! { x: 200.0, y: 10.0 }].map(from_lonlat)),
        );
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

    /// Per record, its tile and its geometry moved back to the zoom's global grid.
    fn global_pieces(
        layer: &RenderLayer,
        geom: FeatureGeom<'_>,
    ) -> Vec<((u32, u32), GeomKind, Vec<Vertex>)> {
        let extent = i32::try_from(layer.grid.extent).unwrap();
        render_in(
            TileOrder::Tms,
            layer,
            &Feature {
                id: Some(1),
                geom,
                props: &EncodedProps::default(),
                bands: &[],
                sort: None,
                zooms: layer.zooms.clone(),
                simplify: PixelThreshold::ZERO,
                min_size: PixelThreshold::ZERO,
            },
        )
        .into_iter()
        .map(|(id, kind, geom)| {
            let c = TileOrder::Tms.tile_coord(TileId::new(id).unwrap()).unwrap();
            let (ox, oy) = (
                i32::try_from(c.x()).unwrap() * extent,
                i32::try_from(c.y()).unwrap() * extent,
            );
            let global = geom.vertices.iter().map(|&[x, y]| [x + ox, y + oy]);
            ((c.x(), c.y()), kind, global.collect())
        })
        .collect()
    }

    #[test]
    fn unclipped_features_are_whole_in_the_tiles_clipped_ones_reach() {
        let clipped = RenderLayer::new(0, 3..=3, GRID).unwrap();
        let unclipped = clipped.clone().with_clip_mode(ClipMode::Unclipped);
        let line = LineString::from(vec![(0.1, 0.12), (0.5, 0.6), (0.9, 0.52)]);
        let polygon = Polygon::new(square(0.1, 0.9), vec![square(0.3, 0.4)]);
        let polygons = std::slice::from_ref(&polygon);
        let lines = FeatureGeom::Lines(std::slice::from_ref(&line));
        for geom in [lines, FeatureGeom::Polygons(polygons)] {
            let pieces = global_pieces(&unclipped, geom);
            let clipped_pieces = global_pieces(&clipped, geom);
            assert!(pieces.len() > 4, "{} pieces", pieces.len());
            assert!(pieces.iter().all(|p| p.2 == pieces[0].2), "always whole");
            assert!(clipped_pieces.iter().any(|p| p.2 != clipped_pieces[0].2));
        }
        let tiles = |layer| {
            let mut tiles: Vec<_> = global_pieces(layer, lines).iter().map(|p| p.0).collect();
            let len = tiles.len();
            tiles.dedup();
            (tiles, len)
        };
        let (line_tiles, records) = tiles(&unclipped);
        assert_eq!(records, line_tiles.len(), "one piece per tile");
        assert_eq!(line_tiles, tiles(&clipped).0);

        let (tiles, records) = polygon_tiles(TileOrder::Tms, &unclipped, polygons);
        assert_eq!(
            tiles,
            polygon_tiles(TileOrder::Tms, &clipped, polygons).0,
            "covered tiles get the whole polygon"
        );
        assert_eq!(records, tiles.len(), "no fill ranges");
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
            &Feature {
                id: Some(1),
                geom: FeatureGeom::Polygons(std::slice::from_ref(&polygon)),
                props: &EncodedProps::default(),
                bands: &[],
                sort: None,
                zooms: 4..=4,
                simplify: PixelThreshold::DEFAULT_SIMPLIFY,
                min_size: PixelThreshold::DEFAULT_MIN_SIZE,
            },
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
        let layer = RenderLayer::new(0, 1..=1, GRID)
            .unwrap()
            .with_bounds(Bounds::new(-180.0, 10.0, -40.0, 80.0));
        let line = LineString::from(vec![coord! { x: 0.1, y: 0.1 }, coord! { x: 0.9, y: 0.1 }]);
        let tiles = render_feature(
            &layer,
            &Feature {
                id: Some(1),
                geom: FeatureGeom::Lines(std::slice::from_ref(&line)),
                props: &EncodedProps::default(),
                bands: &[],
                sort: None,
                zooms: 1..=1,
                simplify: PixelThreshold::DEFAULT_SIMPLIFY,
                min_size: PixelThreshold::ZERO,
            },
        );
        assert_eq!(
            tiles.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(),
            ["1/0/0"]
        );
    }

    #[test]
    fn unquantizable_points_are_skipped() {
        let layer = RenderLayer::new(0, 0..=1, GRID).unwrap();
        let tiles = render(
            &layer,
            FeatureGeom::Points(&[coord! { x: f64::NAN, y: 0.5 }]),
        );
        let names: Vec<_> = tiles.iter().map(|t| t.0.as_str()).collect();
        assert_eq!(names, Vec::<&str>::new());
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

    #[test]
    fn unrenderable_zooms_are_counted_per_layer_with_the_feature() {
        let layer = RenderLayer::new(3, 0..=2, GRID).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let sorter = Sorter::new(SortConfig {
            temp_dirs: vec![dir.path().to_path_buf()],
            buffer_bytes: 1 << 20,
            max_fan_in: 8,
            read_buffer_bytes: 4096,
        })
        .unwrap();
        let mut buffer = sorter.buffer();
        let mut renderer = Renderer::default();
        renderer
            .render(
                TileOrder::Tms,
                &layer,
                Seq::new(4, 7).unwrap(),
                &Feature {
                    id: Some(42),
                    geom: FeatureGeom::Points(&[coord! { x: 0.5, y: f64::NAN }]),
                    props: &EncodedProps::default(),
                    bands: &[],
                    sort: None,
                    zooms: 0..=2,
                    simplify: PixelThreshold::DEFAULT_SIMPLIFY,
                    min_size: PixelThreshold::DEFAULT_MIN_SIZE,
                },
                &mut buffer,
            )
            .unwrap();
        buffer.finish().unwrap();

        assert_eq!(renderer.slice_errors.len(), 4);
        assert!(renderer.slice_errors[..3].iter().all(|e| e.count == 0));
        let errors = &renderer.slice_errors[3];
        assert_eq!(errors.count, 3);
        let first = errors.first.as_ref().unwrap();
        assert_eq!(
            (first.seq, first.id, first.zoom),
            (Seq::new(4, 7).unwrap(), Some(42), 2)
        );
        assert!(matches!(first.error, GeometryError::CoordOverflow));
    }

    #[test]
    fn merged_slice_errors_keep_the_lowest_seq() {
        let error = |partition, zoom| SliceError {
            seq: Seq::new(partition, 0).unwrap(),
            id: None,
            zoom,
            error: GeometryError::CoordOverflow,
        };
        let mut total = SliceErrors {
            count: 2,
            first: Some(error(5, 10)),
        };
        total.merge(SliceErrors {
            count: 1,
            first: Some(error(2, 12)),
        });
        total.merge(SliceErrors {
            count: 4,
            first: Some(error(3, 14)),
        });
        total.merge(SliceErrors::default());

        assert_eq!(total.count, 7);
        assert_eq!(total.first.unwrap().zoom, 12);
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
        while let Some((key, bytes)) = merger.next_record().unwrap() {
            out.push((key.tile_id().value(), bytes.to_vec()));
        }
        out
    }

    #[test]
    #[expect(clippy::cast_possible_truncation, reason = "the hull of i32 vertices")]
    fn sole_tile_features_match_the_slicer() {
        let layer = RenderLayer::new(0, 4..=4, GRID).unwrap();
        let feature = Feature {
            id: Some(7),
            geom: FeatureGeom::Points(&[]),
            props: &EncodedProps::default(),
            bands: &[],
            sort: None,
            zooms: 0..=u8::MAX,
            simplify: PixelThreshold::DEFAULT_SIMPLIFY,
            min_size: PixelThreshold::DEFAULT_MIN_SIZE,
        };
        let ctx = Ctx {
            order: TileOrder::Tms,
            layer: &layer,
            seq: Seq::default(),
            feature: &feature,
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
