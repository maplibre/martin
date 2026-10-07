//! Renders one feature into tile pieces for every zoom of its layer, pushed straight into a sort buffer.
//! Per zoom: size filter, simplification, quantization to the zoom grid, slicing, one record per tile.

use std::ops::RangeInclusive;

use geo::SimplifyIdx as _;
use geo_types::{Coord, LineString};
use map_tile_toolkit::{SlicerAll, TileError};
use martin_tile_utils::TileCoord;

use crate::props::{KeyId, PropRef};
use crate::record::{EncodedProps, Geom, Vertex, encode};
use crate::{LayerGrid, Seq, SortBuffer, SortKey, TileGenError, TileGenResult, TileOrder};

/// Simplification and size thresholds are in pixels of a 256-pixel tile, as in Planetiler.
const TILE_PIXELS: f64 = 256.0;
/// The toolkit indexes polyline vertices with 16 bits.
const MAX_SLICED_VERTICES: usize = 1 << 16;

/// A feature in Web Mercator unit coordinates (see [`project`](crate::project)).
pub struct Feature<'a> {
    pub id: Option<u64>,
    pub geom: FeatureGeom<'a>,
    pub props: &'a [(KeyId, PropRef<'a>)],
}

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
    /// RDP tolerance in pixels below the layer's max zoom, and at it.
    pub simplify: (f64, f64),
    /// Lines and polygons whose bounding box is smaller than this many pixels are dropped, below the
    /// max zoom and at it. Size only shrinks with zoom, so a dropped feature stays dropped.
    pub min_size: (f64, f64),
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
        })
    }

    fn at_max(&self, zoom: u8, (below, at): (f64, f64)) -> f64 {
        if zoom == *self.zooms.end() { at } else { below }
    }
}

/// Per worker; every buffer is reused across features.
#[derive(Default)]
pub struct Renderer {
    slicers: Vec<(LayerGrid, SlicerAll)>,
    props: EncodedProps,
    kept: Vec<usize>,
    /// The quantized feature in the zoom's global grid: line lengths and vertices.
    parts: Vec<u32>,
    vertices: Vec<Coord<i32>>,
    /// One tile's piece: lengths and tile-local vertices.
    piece_parts: Vec<u32>,
    piece: Vec<Vertex>,
    points: Vec<(i32, i32, Vertex)>,
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
                let Some(bbox) = bbox(lines.iter().flat_map(|l| &l.0)) else {
                    return Ok(());
                };
                let shift = world_shift([bbox.0, bbox.1].iter());
                for zoom in layer.zooms.clone().rev() {
                    let size =
                        (bbox.1.x - bbox.0.x).max(bbox.1.y - bbox.0.y) * TILE_PIXELS * scale(zoom);
                    if size < layer.at_max(zoom, layer.min_size) {
                        break;
                    }
                    match self.render_lines(&ctx, zoom, lines, shift, out) {
                        Err(TileGenError::Slice(_)) => self.slice_errors += 1,
                        other => other?,
                    }
                    self.slicer(layer.grid)?.clear();
                }
            }
        }
        Ok(())
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
            self.piece.extend(rest[..len].iter().map(|p| p.2));
            rest = &rest[len..];
            ctx.push(zoom, tx, ty, &self.props, Geom::Points(&self.piece), out)?;
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
        // RDP is scale-invariant: simplify in unit coordinates with the tolerance scaled to them.
        let epsilon = ctx.layer.at_max(zoom, ctx.layer.simplify) / (TILE_PIXELS * scale(zoom));
        self.parts.clear();
        self.vertices.clear();
        for line in lines {
            self.kept = line.simplify_idx(epsilon);
            let start = self.vertices.len();
            for &i in &self.kept {
                let [x, y] = quantize(line.0[i], shift, world)?;
                let c = Coord { x, y };
                if self.vertices.len() == start || self.vertices.last() != Some(&c) {
                    self.vertices.push(c);
                }
            }
            match self.vertices.len() - start {
                0 => {}
                1 => self.vertices.truncate(start),
                n => self
                    .parts
                    .push(u32::try_from(n).map_err(|_overflow| TileGenError::RecordTooLarge(n))?),
            }
        }

        let (parts, vertices) = (
            std::mem::take(&mut self.parts),
            std::mem::take(&mut self.vertices),
        );
        let result = self.slice_lines(ctx, zoom, &parts, &vertices, out);
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
        let mut rest = vertices;
        for &len in parts {
            let (line, tail) = rest.split_at(len as usize);
            rest = tail;
            // Consecutive chunks share a vertex, so the line stays connected.
            let mut start = 0;
            while start + 1 < line.len() {
                let end = (start + MAX_SLICED_VERTICES).min(line.len());
                slicer.add_feature(&line[start..end])?;
                start = end - 1;
            }
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
            self.piece_parts.clear();
            self.piece.clear();
            if ctx.layer.clip {
                for polyline in tile.iter_features().flat_map(|f| f.iter_polylines()) {
                    self.piece_parts.push(
                        u32::try_from(polyline.len())
                            .map_err(|_overflow| TileGenError::RecordTooLarge(polyline.len()))?,
                    );
                    self.piece.extend(polyline.iter().map(|c| [c.x, c.y]));
                }
            } else {
                let (ox, oy) = (id.x * extent, id.y * extent);
                self.piece_parts.extend_from_slice(parts);
                self.piece
                    .extend(vertices.iter().map(|c| [c.x - ox, c.y - oy]));
            }
            let geom = Geom::Lines {
                parts: &self.piece_parts,
                vertices: &self.piece,
            };
            ctx.push(zoom, id.x, id.y, &self.props, geom, out)?;
        }
        Ok(())
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
        if y >= side.cast_unsigned() {
            return Ok(());
        }
        let tile = self.order.tile_id(TileCoord::new_unchecked(zoom, x, y))?;
        out.push_with(SortKey::new(tile, self.layer.index, self.seq), |buf| {
            encode(buf, self.id, props, geom);
        })
    }
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
            .render(TileOrder::Tms, layer, Seq::default(), &feature, &mut buffer)
            .unwrap();
        buffer.finish().unwrap();
        let mut merger = sorter.merge().unwrap();
        let mut out = Vec::new();
        while let Some((key, bytes)) = merger.next().unwrap() {
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
