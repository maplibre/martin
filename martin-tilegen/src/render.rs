//! Renders one feature into tile pieces for every zoom it has in its layer, pushed straight into a sort buffer.

use std::ops::RangeInclusive;

use geo::{BoundingRect, SimplifyIdx as _};
use geo_types::{Coord, LineString, Rect};
use map_tile_toolkit::SlicerAll;
use martin_tile_utils::{TileCoord, TileRect, bbox_to_xyz};
use tilejson::Bounds;

use crate::record::{EncodedProps, Geom, Vertex, encode};
use crate::{LayerGrid, LayerId, Seq, SortBuffer, SortKey, TileGenError, TileGenResult, TileOrder};

/// Simplification and size thresholds are in pixels of a 256-pixel tile, as in Planetiler.
const TILE_PIXELS: f64 = 256.0;
/// The toolkit indexes polyline vertices with 16 bits.
const MAX_SLICED_VERTICES: usize = 1 << 16;

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
}

#[derive(Clone, Debug)]
pub struct RenderLayer {
    /// The layer byte of the sort key.
    pub index: u8,
    pub zooms: RangeInclusive<u8>,
    pub grid: LayerGrid,
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
    /// A layer keeping every tile in `zooms`.
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
            tiles: None,
        })
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
        let Some(tiles) = &self.tiles else {
            return true;
        };
        let rect = tiles[usize::from(zoom - self.zooms.start())];
        (rect.min_x..=rect.max_x).contains(&x) && (rect.min_y..=rect.max_y).contains(&y)
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
    slicers: Vec<(LayerGrid, SlicerAll)>,
    parts: Vec<u32>,
    vertices: Vec<Coord<i32>>,
    piece_parts: Vec<u32>,
    piece: Vec<Vertex>,
    points: Vec<(i32, i32, Vertex)>,
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
                let Some(bbox) = lines
                    .iter()
                    .filter_map(BoundingRect::bounding_rect)
                    .reduce(union)
                else {
                    return Ok(());
                };
                let shift = world_shift(bbox);
                for zoom in zooms.rev() {
                    let size = bbox.width().max(bbox.height()) * TILE_PIXELS * scale(zoom);
                    if size < layer.pixels(zoom, ctx.feature.min_size) {
                        break;
                    }
                    let result = self.render_lines(&ctx, zoom, lines, shift, out);
                    self.skip_unrenderable(&ctx, zoom, result)?;
                }
            }
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
            self.piece.extend(group.iter().map(|p| p.2));
            ctx.push(zoom, tx, ty, Geom::Points(&self.piece), out)?;
        }
        Ok(())
    }

    /// Simplifies and quantizes the lines at `zoom`, slices them into tiles and pushes one piece per
    /// tile. Called once per feature and zoom.
    fn render_lines(
        &mut self,
        ctx: &Ctx<'_>,
        zoom: u8,
        lines: &[LineString<f64>],
        shift: f64,
        out: &mut SortBuffer<'_>,
    ) -> TileGenResult<()> {
        let grid = ctx.layer.grid;
        let world = scale(zoom) * f64::from(grid.extent);
        let epsilon = ctx.layer.pixels(zoom, ctx.feature.simplify) / (TILE_PIXELS * scale(zoom));
        self.parts.clear();
        self.vertices.clear();
        for line in lines {
            let start = self.vertices.len();
            for i in line.simplify_idx(epsilon) {
                let [x, y] = quantize(line.0[i], shift, world)?;
                let c = Coord { x, y };
                if self.vertices.len() == start || self.vertices.last() != Some(&c) {
                    self.vertices.push(c);
                }
            }
            match self.vertices.len() - start {
                0 => {}
                1 => self.vertices.truncate(start),
                n => self.parts.push(vertex_count(n)?),
            }
        }

        let slicer_index = self.slicer_index(ctx.layer)?;
        let slicer = &mut self.slicers[slicer_index].1;
        slicer.clear();
        let mut rest = self.vertices.as_slice();
        for &len in &self.parts {
            let (line, tail) = rest.split_at(len as usize);
            rest = tail;
            let mut start = 0;
            while start + 1 < line.len() {
                let end = (start + MAX_SLICED_VERTICES).min(line.len());
                slicer
                    .add_feature(&line[start..end])
                    .map_err(GeometryError::from)?;
                start = end - 1;
            }
        }
        for tile in slicer.iter_tiles() {
            let id = tile.tile_id();
            self.piece_parts.clear();
            self.piece.clear();
            for polyline in tile.iter_features().flat_map(|f| f.iter_polylines()) {
                self.piece_parts.push(vertex_count(polyline.len())?);
                self.piece.extend(polyline.iter().map(|c| [c.x, c.y]));
            }
            let geom = Geom::Lines {
                parts: &self.piece_parts,
                vertices: &self.piece,
            };
            ctx.push(zoom, id.x, id.y, geom, out)?;
        }
        Ok(())
    }

    /// The slicer for the layer's grid, created on first use and reused across features.
    fn slicer_index(&mut self, layer: &RenderLayer) -> TileGenResult<usize> {
        let grid = layer.grid;
        if let Some(pos) = self.slicers.iter().position(|(g, _)| *g == grid) {
            return Ok(pos);
        }
        let buffer = u16::try_from(grid.buffer).map_err(|_too_large| invalid_buffer(grid))?;
        self.slicers.push((
            grid,
            SlicerAll::new(grid.extent, buffer).map_err(GeometryError::from)?,
        ));
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
        let tile = self.order.tile_id(TileCoord::new_unchecked(zoom, x, y))?;
        out.push_with(
            SortKey::new(tile, LayerId::new(self.layer.index), self.seq),
            |buf| {
                encode(buf, self.feature.id, self.feature.props, geom);
            },
        )
    }
}

/// Tiles per world side at `zoom`.
fn scale(zoom: u8) -> f64 {
    f64::from(1u32 << zoom)
}

fn to_i32(v: u32) -> Result<i32, GeometryError> {
    i32::try_from(v).map_err(|_overflow| GeometryError::CoordOverflow)
}

fn vertex_count(n: usize) -> Result<u32, GeometryError> {
    u32::try_from(n).map_err(|_overflow| GeometryError::TooManyVertices(n))
}

fn invalid_buffer(grid: LayerGrid) -> RenderLayerError {
    RenderLayerError::InvalidBuffer {
        buffer: grid.buffer,
        extent: grid.extent,
    }
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
            .render(TileOrder::Tms, layer, Seq::default(), feature, &mut buffer)
            .unwrap();
        buffer.finish().unwrap();
        let mut merger = sorter.merge().unwrap();
        let mut out = Vec::new();
        while let Some((key, bytes)) = merger.next_record().unwrap() {
            let c = TileOrder::Tms.tile_coord(key.tile_id()).unwrap();
            let record = Record::decode(bytes).unwrap();
            let mut geom = GeomBuf::default();
            record.append_geometry(&mut geom).unwrap();
            out.push((format!("{c:#}"), record.kind, geom));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
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
}
