//! Per-tile operations: each takes one layer's features in one tile, in draw order, and returns the new list.

mod limit;
mod merge_multi;
mod thinning;

use std::cell::RefCell;
use std::ops::Range;

use mlt_core::PropValueRef;

use crate::plan::{PlannedOp, PlannedTileOp};
use crate::props::{KeyId, KeyNames};
use crate::sort::Seq;
use crate::sort::record::{GeomBuf, GeomKind, Record, Vertex};
use crate::{LayerGrid, TileGenResult};

/// One feature of a layer in a tile, as ranges into the records and properties the assembler decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TileFeature {
    /// The first of the feature's pieces in this tile; for a merged feature, that of its first member.
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
    /// A geometry an operation made by merging features, by index into [`LayerTile::merged`].
    Merged(u32),
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

/// What merging several features made: one geometry and the id they shared, if they did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Merged {
    /// [`GeomKind::Point`], [`GeomKind::Line`] or [`GeomKind::Polygon`].
    pub(crate) kind: GeomKind,
    pub(crate) geom: GeomBuf,
    pub(crate) id: Option<u64>,
}

/// One layer's decoded records and properties in a tile, which [`TileFeature`]s point into.
pub(crate) struct LayerTile<'r, 'a> {
    pub(crate) decoded: &'r [(Seq, Record<'a>)],
    /// Operations that keep only some of a feature's properties append those here.
    pub(crate) props: RefCell<Vec<(KeyId, PropValueRef<'a>)>>,
    pub(crate) names: &'r KeyNames,
    pub(crate) grid: LayerGrid,
    /// What merges made, which [`Shape::Merged`] points into.
    pub(crate) merged: RefCell<Vec<Merged>>,
}

impl LayerTile<'_, '_> {
    /// Fills `out` with the feature's geometry in tile coordinates, a fill as the square of the tile and its
    /// buffer, and returns its kind and id.
    pub(crate) fn geometry(&self, feature: &TileFeature, out: &mut GeomBuf) -> TileGenResult<(GeomKind, Option<u64>)> {
        out.clear();
        let first = &self.decoded[feature.first()].1;
        match feature.shape {
            Shape::Merged(index) => {
                let merged = &self.merged.borrow()[index as usize];
                out.polygons.extend_from_slice(&merged.geom.polygons);
                out.parts.extend_from_slice(&merged.geom.parts);
                out.vertices.extend_from_slice(&merged.geom.vertices);
                Ok((merged.kind, merged.id))
            }
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

    /// Stores `merged` and makes `feature` draw it, returning its index in [`Self::merged`].
    pub(crate) fn set_merged(&self, feature: &mut TileFeature, merged: Merged) -> u32 {
        let mut all = self.merged.borrow_mut();
        let index = narrow(all.len());
        feature.shape = Shape::Merged(index);
        all.push(merged);
        index
    }

    /// The feature's kind as merges see it: points, lines or polygons, a fill being a polygon.
    pub(crate) fn base_kind(&self, feature: &TileFeature) -> GeomKind {
        if let Shape::Merged(index) = feature.shape {
            return self.merged.borrow()[index as usize].kind;
        }
        match self.decoded[feature.first()].1.kind {
            GeomKind::Point | GeomKind::GriddedPoint { .. } => GeomKind::Point,
            GeomKind::Line => GeomKind::Line,
            GeomKind::Polygon | GeomKind::Fill | GeomKind::FillRange { .. } => GeomKind::Polygon,
        }
    }

    /// Makes `feature` keep only its properties named in `names`, in their order.
    pub(crate) fn keep_props(&self, feature: &mut TileFeature, names: &[String]) {
        let mut props = self.props.borrow_mut();
        let start = props.len();
        for index in feature.props() {
            let (key, value) = props[index];
            if names.iter().any(|name| self.names.name(key) == name) {
                props.push((key, value));
            }
        }
        feature.props_first = narrow(start);
        feature.props_end = narrow(props.len());
    }

    /// The values merges group features by: those of `group_by`, by name, a missing one as `null`; or
    /// every property, by key. Values compare by type too, so `1` and `"1"` differ.
    pub(crate) fn group_key(&self, feature: &TileFeature, group_by: Option<&[String]>) -> Vec<GroupValue> {
        let props = self.props.borrow();
        let props = &props[feature.props()];
        if let Some(names) = group_by {
            names
                .iter()
                .map(|name| {
                    props
                        .iter()
                        .find(|(key, _)| self.names.name(*key) == name)
                        .map_or(GroupValue::Null, |&(_, value)| GroupValue::from(value))
                })
                .collect()
        } else {
            let mut values: Vec<_> = props
                .iter()
                .map(|&(key, value)| GroupValue::Keyed(key, Box::new(GroupValue::from(value))))
                .collect();
            values.sort();
            values
        }
    }
}

/// A property value as merges compare it: ordered, with floats by their bits.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum GroupValue {
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    F32(u32),
    F64(u64),
    Str(String),
    /// A value under its key, when every property takes part.
    Keyed(KeyId, Box<Self>),
}

impl From<PropValueRef<'_>> for GroupValue {
    fn from(value: PropValueRef<'_>) -> Self {
        match value {
            PropValueRef::Bool(v) => Self::Bool(v),
            PropValueRef::I8(v) => Self::Int(v.into()),
            PropValueRef::U8(v) => Self::Int(v.into()),
            PropValueRef::I32(v) => Self::Int(v.into()),
            PropValueRef::U32(v) => Self::Int(v.into()),
            PropValueRef::I64(v) => Self::Int(v),
            PropValueRef::U64(v) => Self::Uint(v),
            PropValueRef::F32(v) => Self::F32(v.to_bits()),
            PropValueRef::F64(v) => Self::F64(v.to_bits()),
            PropValueRef::Str(v) => Self::Str(v.to_owned()),
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
    /// Combines a group's features of one type into a multi-feature.
    MergeMulti,
    /// Keeps the first features.
    Limit,
}

impl OpKind {
    /// Every operation, in the order they run.
    pub const ALL: [Self; 3] = [Self::Thinning, Self::MergeMulti, Self::Limit];

    /// The operation's key in the config.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Thinning => "thinning",
            Self::MergeMulti => "merge_multi",
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
                PlannedOp::MergeMulti { geometry, group_by } => {
                    merge_multi::apply(tile, geometry, group_by, features)?;
                }
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
