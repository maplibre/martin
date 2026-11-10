//! Per-tile operations: each takes one layer's features in one tile, in draw order, and returns the new list.

mod limit;
mod merge_lines;
mod merge_multi;
mod merge_polygons;
mod thinning;

use std::cell::RefCell;
use std::ops::Range;

use geo_types::{Coord, LineString, MultiPolygon, Polygon};
use mlt_core::PropValueRef;

use crate::expr::{ExprKeys, PropSlots};
use crate::phase2::per_tile::thinning::Thinning;
use crate::plan::{Plan, PlannedOp, PlannedTileOp};
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

/// The lines of a line feature's geometry.
pub(crate) fn lines(geom: &GeomBuf) -> Vec<LineString<f64>> {
    paths(&geom.parts, &geom.vertices).collect()
}

/// The polygons of a polygon feature's geometry, its rings closed.
pub(crate) fn polygons(geom: &GeomBuf) -> MultiPolygon<f64> {
    let mut rings = paths(&geom.parts, &geom.vertices);
    geom.polygons
        .iter()
        .filter_map(|&count| {
            let mut polygon = rings.by_ref().take(count as usize);
            let exterior = polygon.next()?;
            Some(Polygon::new(exterior, polygon.collect()))
        })
        .collect()
}

/// Consecutive runs of `vertices`, `parts` long each, as line strings.
fn paths<'g>(parts: &'g [u32], mut vertices: &'g [Vertex]) -> impl Iterator<Item = LineString<f64>> + 'g {
    parts.iter().map(move |&len| {
        let (part, rest) = vertices.split_at(len as usize);
        vertices = rest;
        part.iter()
            .map(|&[x, y]| Coord::from((f64::from(x), f64::from(y))))
            .collect()
    })
}

/// The tile and its buffer as one square polygon, which a fill stands for.
pub(crate) fn push_square(geom: &mut GeomBuf, grid: LayerGrid) {
    #[expect(clippy::cast_possible_wrap, reason = "extent + buffer are far below i32::MAX")]
    let (lo, hi) = (-(grid.buffer as i32), (grid.extent + grid.buffer) as i32);
    geom.polygons.push(1);
    geom.parts.push(4);
    geom.vertices.extend([[lo, lo], [hi, lo], [hi, hi], [lo, hi]]);
}

/// What per-tile operations evaluate expressions with, for every layer of a plan.
pub struct OutputExprs<'p> {
    plan: &'p Plan,
    /// Each layer's output attribute names, as its expressions read them.
    keys: Vec<ExprKeys>,
}

impl<'p> OutputExprs<'p> {
    /// The expressions of `plan`, over output attributes named by `names`, one per layer.
    pub(crate) fn new(plan: &'p Plan, names: &[KeyNames]) -> Self {
        Self {
            plan,
            keys: names
                .iter()
                .map(|names| names.iter().map(|(key, name)| (name, key)).collect())
                .collect(),
        }
    }

    /// What the operations of layer `layer` evaluate expressions with.
    pub(crate) fn layer(&self, layer: usize) -> LayerExprs<'_> {
        LayerExprs {
            plan: self.plan,
            keys: &self.keys[layer],
        }
    }
}

/// What one layer's per-tile operations evaluate expressions with.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LayerExprs<'a> {
    pub(crate) plan: &'a Plan,
    pub(crate) keys: &'a ExprKeys,
}

/// A per-tile operation, in the order they run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OpKind {
    /// Joins a group's lines whose ends meet.
    MergeLines,
    /// Unions a group's polygons.
    MergePolygons,
    /// Drops features an earlier kept feature with the same key crowds out.
    Thinning,
    /// Combines a group's features of one type into a multi-feature.
    MergeMulti,
    /// Keeps the first features.
    Limit,
}

impl OpKind {
    /// Every operation, in the order they run.
    pub const ALL: [Self; 5] = [
        Self::MergeLines,
        Self::MergePolygons,
        Self::Thinning,
        Self::MergeMulti,
        Self::Limit,
    ];

    /// The operation's key in the config.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::MergeLines => "merge_lines",
            Self::MergePolygons => "merge_polygons",
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
    /// Groups a merge could not make valid, which kept their features.
    pub groups_failed: u64,
}

impl OpCounts {
    /// Adds `other` to these counts.
    pub fn add(&mut self, other: &Self) {
        self.tiles += other.tiles;
        self.features_in += other.features_in;
        self.features_out += other.features_out;
        self.groups_failed += other.groups_failed;
    }

    /// One run on a tile, from `features_in` to `features_out` features.
    fn run(features_in: usize, features_out: usize, groups_failed: u64) -> Self {
        Self {
            tiles: 1,
            features_in: features_in as u64,
            features_out: features_out as u64,
            groups_failed,
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

/// Per-worker scratch of the operations, the failed evaluations of each plan expression, and what the
/// operations did to the last layer they ran on.
#[derive(Debug, Default)]
pub(crate) struct OpScratch {
    pub(crate) slots: PropSlots,
    /// Failed evaluations by plan expression index.
    pub(crate) errors: Vec<u64>,
    /// Reset for each layer of each tile.
    pub(crate) counts: LayerOpCounts,
    pub(crate) thinning: thinning::Scratch,
}

/// A layer's per-tile operations, and the zoom of the tile they run on.
#[derive(Clone, Copy, Debug)]
pub struct TileOps<'a> {
    ops: &'a [PlannedTileOp],
    zoom: u8,
    exprs: Option<LayerExprs<'a>>,
}

impl<'a> TileOps<'a> {
    /// No operations, so every feature passes through.
    #[cfg(any(test, feature = "internals"))]
    pub const NONE: Self = Self {
        ops: &[],
        zoom: 0,
        exprs: None,
    };

    /// `ops` in the order they run, on a tile of zoom `zoom`, with expressions evaluated with `exprs`.
    #[must_use]
    pub(crate) fn new(ops: &'a [PlannedTileOp], zoom: u8, exprs: Option<LayerExprs<'a>>) -> Self {
        Self { ops, zoom, exprs }
    }

    /// The operations that run at this zoom, in order.
    fn active(self) -> impl Iterator<Item = &'a PlannedOp> {
        self.ops
            .iter()
            .filter(move |def| def.zooms.contains(&self.zoom))
            .map(|def| &def.op)
    }

    /// Runs the operations of this zoom on `features`, in draw order. Thinning runs in its place even
    /// without a thinning of the layer at this zoom, for the points that rules gave a grid.
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
                self.thin(tile, None, features, scratch)?;
            }
            let features_in = features.len();
            let mut groups_failed = 0;
            match op {
                PlannedOp::MergeLines(op) => merge_lines::apply(tile, op, features)?,
                PlannedOp::MergePolygons(op) => groups_failed = merge_polygons::apply(tile, op, features)?,
                PlannedOp::Thinning { keys, method } => {
                    thinned = true;
                    self.thin(tile, Some(Thinning { keys, method: *method }), features, scratch)?;
                    continue;
                }
                PlannedOp::MergeMulti { geometry, group_by } => {
                    merge_multi::apply(tile, geometry, group_by, features)?;
                }
                PlannedOp::Limit { count } => limit::apply(*count, features),
            }
            scratch
                .counts
                .record(op.kind(), &OpCounts::run(features_in, features.len(), groups_failed));
        }
        if !thinned {
            self.thin(tile, None, features, scratch)?;
        }
        Ok(())
    }

    /// Runs `thinning`, and the grids rules gave points, and counts what they did.
    fn thin(
        self,
        tile: &LayerTile<'_, '_>,
        thinning: Option<Thinning<'_>>,
        features: &mut Vec<TileFeature>,
        scratch: &mut OpScratch,
    ) -> TileGenResult<()> {
        let OpScratch {
            slots,
            errors,
            counts,
            thinning: thin_scratch,
        } = scratch;
        let ran = thinning::apply(
            tile,
            self.exprs,
            thinning,
            self.zoom,
            features,
            thin_scratch,
            slots,
            errors,
        )?;
        if let Some(ran) = ran {
            counts.record(OpKind::Thinning, &ran);
        }
        Ok(())
    }
}
