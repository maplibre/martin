//! Renders one feature into tile pieces for every zoom it has in its layer, pushed straight into a sort buffer.

use std::ops::RangeInclusive;

use geo::SimplifyIdx as _;
use geo_types::{Coord, LineString};
use map_tile_toolkit::SlicerAll;
use martin_tile_utils::TileCoord;

use crate::record::{EncodedProps, Geom, Vertex, encode};
use crate::{LayerGrid, LayerId, Seq, SortBuffer, SortKey, TileGenError, TileGenResult, TileOrder};

/// Simplification and size thresholds are in pixels of a 256-pixel tile, as in Planetiler.
const TILE_PIXELS: f64 = 256.0;
/// The toolkit indexes polyline vertices with 16 bits.
const MAX_SLICED_VERTICES: usize = 1 << 16;

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
    /// `false` keeps whole features in every tile they touch, like `ST_AsMVTGeom(..., clip_geom => false)`.
    pub clip: bool,
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

    /// Planetiler's default simplification: 0.1 px, and 1/16 px at the max zoom.
    pub const PLANETILER_SIMPLIFY: Self = Self {
        below_max_zoom: 0.1,
        at_max_zoom: 0.0625,
    };

    /// Planetiler's default minimum size: 1 px, and 1/16 px at the max zoom.
    pub const PLANETILER_MIN_SIZE: Self = Self {
        below_max_zoom: 1.0,
        at_max_zoom: 0.0625,
    };
}

impl RenderLayer {
    /// Clipped.
    pub fn new(index: u8, zooms: RangeInclusive<u8>, grid: LayerGrid) -> TileGenResult<Self> {
        let max_zoom = *zooms.end();
        if zooms.is_empty() {
            return Err(TileGenError::EmptyZooms(index));
        }
        let fits = u64::from(grid.extent)
            .checked_shl(u32::from(max_zoom))
            .is_some_and(|s| s <= 1 << 30);
        if !fits {
            return Err(TileGenError::ZoomGridOverflow {
                index,
                extent: grid.extent,
                max_zoom,
            });
        }
        if grid.buffer >= grid.extent / 2 || u16::try_from(grid.buffer).is_err() {
            return Err(invalid_buffer(index, grid));
        }
        Ok(Self {
            index,
            zooms,
            grid,
            clip: true,
        })
    }

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
    /// Features skipped at a zoom because they could not be quantized or sliced.
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
        let ctx = Ctx {
            order,
            layer,
            seq,
            id: feature.id,
            props: feature.props,
            simplify: feature.simplify,
            min_size: feature.min_size,
        };
        let zooms = (*feature.zooms.start()).max(*layer.zooms.start())
            ..=(*feature.zooms.end()).min(*layer.zooms.end());
        match feature.geom {
            FeatureGeom::Points(points) => {
                let shift = world_shift(points.iter());
                for zoom in zooms.rev() {
                    let result = self.render_points(&ctx, zoom, points, shift, out);
                    self.skip_unrenderable(result)?;
                }
            }
            FeatureGeom::Lines(lines) => {
                let Some(bbox) = bbox(lines.iter().flat_map(|l| &l.0)) else {
                    return Ok(());
                };
                let shift = world_shift([bbox.0, bbox.1].iter());
                for zoom in zooms.rev() {
                    let size =
                        (bbox.1.x - bbox.0.x).max(bbox.1.y - bbox.0.y) * TILE_PIXELS * scale(zoom);
                    if size < layer.pixels(zoom, ctx.min_size) {
                        break;
                    }
                    let result = self.render_lines(&ctx, zoom, lines, shift, out);
                    self.skip_unrenderable(result)?;
                }
            }
        }
        Ok(())
    }

    fn skip_unrenderable(&mut self, result: TileGenResult<()>) -> TileGenResult<()> {
        match result {
            Err(TileGenError::Slice(_) | TileGenError::CoordOverflow) => {
                self.slice_errors += 1;
                Ok(())
            }
            other => other,
        }
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
            self.piece.extend(rest[..len].iter().map(|p| p.2));
            rest = &rest[len..];
            ctx.push(zoom, tx, ty, Geom::Points(&self.piece), out)?;
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
        let grid = ctx.layer.grid;
        let world = scale(zoom) * f64::from(grid.extent);
        let epsilon = ctx.layer.pixels(zoom, ctx.simplify) / (TILE_PIXELS * scale(zoom));
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
                slicer.add_feature(&line[start..end])?;
                start = end - 1;
            }
        }
        let extent = to_i32(grid.extent)?;
        for tile in slicer.iter_tiles() {
            let id = tile.tile_id();
            self.piece_parts.clear();
            self.piece.clear();
            if ctx.layer.clip {
                for polyline in tile.iter_features().flat_map(|f| f.iter_polylines()) {
                    self.piece_parts.push(vertex_count(polyline.len())?);
                    self.piece.extend(polyline.iter().map(|c| [c.x, c.y]));
                }
            } else {
                let (ox, oy) = (id.x * extent, id.y * extent);
                self.piece_parts.extend_from_slice(&self.parts);
                self.piece
                    .extend(self.vertices.iter().map(|c| [c.x - ox, c.y - oy]));
            }
            let geom = Geom::Lines {
                parts: &self.piece_parts,
                vertices: &self.piece,
            };
            ctx.push(zoom, id.x, id.y, geom, out)?;
        }
        Ok(())
    }

    fn slicer_index(&mut self, layer: &RenderLayer) -> TileGenResult<usize> {
        let grid = layer.grid;
        if let Some(pos) = self.slicers.iter().position(|(g, _)| same_grid(*g, grid)) {
            return Ok(pos);
        }
        let buffer =
            u16::try_from(grid.buffer).map_err(|_too_large| invalid_buffer(layer.index, grid))?;
        self.slicers
            .push((grid, SlicerAll::new(grid.extent, buffer)?));
        Ok(self.slicers.len() - 1)
    }
}

struct Ctx<'a> {
    order: TileOrder,
    layer: &'a RenderLayer,
    seq: Seq,
    id: Option<u64>,
    props: &'a EncodedProps,
    simplify: PixelThreshold,
    min_size: PixelThreshold,
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
        if y >= side.cast_unsigned() {
            return Ok(());
        }
        let tile = self.order.tile_id(TileCoord::new_unchecked(zoom, x, y))?;
        out.push_with(
            SortKey::new(tile, LayerId::new(self.layer.index), self.seq),
            |buf| {
                encode(buf, self.id, self.props, geom);
            },
        )
    }
}

fn same_grid(a: LayerGrid, b: LayerGrid) -> bool {
    (a.extent, a.buffer) == (b.extent, b.buffer)
}

fn scale(zoom: u8) -> f64 {
    f64::from(1u32 << zoom)
}

fn to_i32(v: u32) -> TileGenResult<i32> {
    i32::try_from(v).map_err(|_overflow| TileGenError::CoordOverflow)
}

fn vertex_count(n: usize) -> TileGenResult<u32> {
    u32::try_from(n).map_err(|_overflow| TileGenError::TooManyVertices(n))
}

fn invalid_buffer(index: u8, grid: LayerGrid) -> TileGenError {
    TileGenError::InvalidBuffer {
        index,
        buffer: grid.buffer,
        extent: grid.extent,
    }
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
        Err(TileGenError::CoordOverflow)
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

    fn render(layer: &RenderLayer, geom: FeatureGeom<'_>) -> Vec<(String, GeomKind, GeomBuf)> {
        render_feature(
            layer,
            &Feature {
                id: Some(1),
                geom,
                props: &EncodedProps::default(),
                zooms: layer.zooms.clone(),
                simplify: PixelThreshold::PLANETILER_SIMPLIFY,
                min_size: PixelThreshold::PLANETILER_MIN_SIZE,
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

    fn unit(lon: f64, lat: f64) -> Coord<f64> {
        let mut c = [coord! { x: lon, y: lat }];
        crate::project::from_lonlat(&mut c);
        c[0]
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
    fn simplification_drops_vertices_at_low_zoom() {
        let layer = RenderLayer::new(0, 0..=8, GRID).unwrap();
        let zigzag: Vec<_> = (0..=100)
            .map(|i| unit(f64::from(i) * 0.01, f64::from(i % 2) * 0.001))
            .collect();
        let tiles = render_feature(
            &layer,
            &Feature {
                id: Some(1),
                geom: FeatureGeom::Lines(&[LineString::from(zigzag)]),
                props: &EncodedProps::default(),
                zooms: 0..=8,
                simplify: PixelThreshold::PLANETILER_SIMPLIFY,
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
}
