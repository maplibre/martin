//! Per-tile operations: each takes one layer's features in one tile, in draw order, and returns the new list.

mod limit;
mod thinning;

use std::ops::Range;

use crate::plan::{PlannedOp, PlannedTileOp};
use crate::sort::Seq;
use crate::sort::record::{GeomBuf, GeomKind, Record, Vertex};
use crate::{LayerGrid, TileGenResult};

/// One feature of a layer in a tile, as ranges into the records and properties the assembler decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TileFeature {
    /// The first of the feature's pieces in this tile.
    first: u32,
    /// One past the last of the feature's pieces.
    end: u32,
    /// The first of the feature's properties.
    props_first: u32,
    /// One past the last of the feature's properties.
    props_end: u32,
    /// What the feature draws.
    pub(crate) shape: Shape,
}

/// What a [`TileFeature`] draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// The geometry of its records.
    Records,
    /// One point of its single record, for a gridded multi-point split into its points.
    Point(Vertex),
}

/// `index` as stored in a [`TileFeature`].
fn narrow(index: usize) -> u32 {
    u32::try_from(index).expect("fewer than 2^32 records and properties in one layer of one tile")
}

impl TileFeature {
    /// The feature drawing `records`, with `props`.
    pub(crate) fn new(records: Range<usize>, props: Range<usize>) -> Self {
        Self {
            first: narrow(records.start),
            end: narrow(records.end),
            props_first: narrow(props.start),
            props_end: narrow(props.end),
            shape: Shape::Records,
        }
    }

    /// The feature's pieces in this tile.
    pub(crate) fn records(&self) -> Range<usize> {
        self.first as usize..self.end as usize
    }

    /// The first of the feature's pieces in this tile.
    pub(crate) fn first(&self) -> usize {
        self.first as usize
    }

    /// The feature's properties.
    pub(crate) fn props(&self) -> Range<usize> {
        self.props_first as usize..self.props_end as usize
    }
}

/// One layer's decoded records in a tile, which [`TileFeature`]s point into.
pub(crate) struct LayerTile<'r, 'a> {
    pub(crate) decoded: &'r [(Seq, Record<'a>)],
    pub(crate) grid: LayerGrid,
}

impl LayerTile<'_, '_> {
    /// Fills `out` with the feature's geometry in tile coordinates, a fill as the square of the tile and its
    /// buffer, and returns its kind and id.
    pub(crate) fn geometry(&self, feature: &TileFeature, out: &mut GeomBuf) -> TileGenResult<(GeomKind, Option<u64>)> {
        out.clear();
        let first = &self.decoded[feature.first()].1;
        match feature.shape {
            Shape::Point(vertex) => {
                out.vertices.push(vertex);
                Ok((first.kind, first.id))
            }
            Shape::Records => {
                for (_, piece) in &self.decoded[feature.records()] {
                    match piece.kind {
                        GeomKind::Fill | GeomKind::FillRange { .. } => push_square(out, self.grid),
                        GeomKind::Point | GeomKind::GriddedPoint { .. } | GeomKind::Line | GeomKind::Polygon => {
                            piece.append_geometry(out)?;
                        }
                    }
                }
                Ok((first.kind, first.id))
            }
        }
    }
}

/// The tile and its buffer as one square polygon, which a fill stands for.
pub(crate) fn push_square(geom: &mut GeomBuf, grid: LayerGrid) {
    #[expect(clippy::cast_possible_wrap, reason = "extent + buffer are far below i32::MAX")]
    let (lo, hi) = (-(grid.buffer as i32), (grid.extent + grid.buffer) as i32);
    geom.polygons.push(1);
    geom.parts.push(4);
    geom.vertices.extend([[lo, lo], [hi, lo], [hi, hi], [lo, hi]]);
}

/// A per-tile operation, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpKind {
    /// Drops features an earlier kept feature crowds out.
    Thinning,
    /// Keeps the first features.
    Limit,
}

impl OpKind {
    /// Every operation, in the order they run.
    pub const ALL: [Self; 2] = [Self::Thinning, Self::Limit];

    /// The operation's key in the config.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Thinning => "thinning",
            Self::Limit => "limit",
        }
    }
}

/// What one operation did to one layer, summed over the tiles it ran on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpCounts {
    /// Tiles of the layer the operation ran on.
    pub tiles: u64,
    /// Features the operation took, over those tiles.
    pub features_in: u64,
    /// Features the operation kept or made, over those tiles.
    pub features_out: u64,
}

impl OpCounts {
    /// Adds `other` to these counts.
    pub fn add(&mut self, other: &Self) {
        self.tiles += other.tiles;
        self.features_in += other.features_in;
        self.features_out += other.features_out;
    }

    /// One run on a tile, from `features_in` to `features_out` features.
    fn run(features_in: usize, features_out: usize) -> Self {
        Self {
            tiles: 1,
            features_in: features_in as u64,
            features_out: features_out as u64,
        }
    }
}

/// The counts of every operation of one layer, by [`OpKind`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LayerOpCounts([OpCounts; OpKind::ALL.len()]);

impl LayerOpCounts {
    /// The counts of operation `kind`.
    #[must_use]
    pub(crate) fn get(&self, kind: OpKind) -> &OpCounts {
        &self.0[kind as usize]
    }

    /// Adds `counts` to operation `kind`.
    pub(crate) fn record(&mut self, kind: OpKind, counts: &OpCounts) {
        self.0[kind as usize].add(counts);
    }

    /// Adds `other` to these counts.
    pub(crate) fn add(&mut self, other: &Self) {
        for (total, counts) in self.0.iter_mut().zip(&other.0) {
            total.add(counts);
        }
    }

    /// Whether no operation ran.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.iter().all(|counts| counts.tiles == 0)
    }
}

/// Per-worker scratch of the operations, and what they did to the last layer they ran on.
#[derive(Debug, Default)]
pub(crate) struct OpScratch {
    /// Reset for each layer of each tile.
    pub(crate) counts: LayerOpCounts,
    pub(crate) thinning: thinning::Scratch,
}

/// A layer's per-tile operations, and the zoom of the tile they run on.
#[derive(Clone, Copy, Debug)]
pub struct TileOps<'a> {
    ops: &'a [PlannedTileOp],
    zoom: u8,
}

impl<'a> TileOps<'a> {
    /// No operations, so every feature passes through.
    #[cfg(any(test, feature = "internals"))]
    pub const NONE: Self = Self { ops: &[], zoom: 0 };

    /// `ops` in the order they run, on a tile of zoom `zoom`.
    #[must_use]
    pub(crate) fn new(ops: &'a [PlannedTileOp], zoom: u8) -> Self {
        Self { ops, zoom }
    }

    /// The operations that run at this zoom, in order.
    fn active(self) -> impl Iterator<Item = &'a PlannedOp> {
        self.ops
            .iter()
            .filter(move |def| def.zooms.contains(&self.zoom))
            .map(|def| &def.op)
    }

    /// Runs the operations of this zoom on `features`, in draw order, with thinning in its place for the
    /// points that rendering put in grid squares.
    pub(crate) fn apply(
        self,
        tile: &LayerTile<'_, '_>,
        features: &mut Vec<TileFeature>,
        scratch: &mut OpScratch,
    ) -> TileGenResult<()> {
        let mut thinned = false;
        for op in self.active() {
            if !thinned && op.kind() > OpKind::Thinning {
                thinned = true;
                self.thin(tile, features, scratch)?;
            }
            let features_in = features.len();
            match op {
                PlannedOp::Limit { count } => limit::apply(*count, features),
            }
            scratch
                .counts
                .record(op.kind(), &OpCounts::run(features_in, features.len()));
        }
        if !thinned {
            self.thin(tile, features, scratch)?;
        }
        Ok(())
    }

    /// Runs the grid thinning and counts what it did.
    fn thin(
        self,
        tile: &LayerTile<'_, '_>,
        features: &mut Vec<TileFeature>,
        scratch: &mut OpScratch,
    ) -> TileGenResult<()> {
        if let Some(ran) = thinning::apply(tile, self.zoom, features, &mut scratch.thinning)? {
            scratch.counts.record(OpKind::Thinning, &ran);
        }
        Ok(())
    }
}
