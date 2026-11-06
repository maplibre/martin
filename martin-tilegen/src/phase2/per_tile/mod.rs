//! Per-tile operations: each takes one layer's features in one tile, in draw order, and returns the new list.

pub(crate) mod label_grid;
mod limit;

use std::ops::Range;

use crate::plan::{TileOp, TileOpDef};
use crate::sort::record::Vertex;

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

/// A layer's per-tile operations, and the zoom of the tile they run on.
#[derive(Clone, Copy, Debug)]
pub struct TileOps<'a> {
    ops: &'a [TileOpDef],
    zoom: u8,
}

impl<'a> TileOps<'a> {
    /// No operations, so every feature passes through.
    #[cfg(any(test, feature = "internals"))]
    pub const NONE: Self = Self { ops: &[], zoom: 0 };

    /// `ops` in the order they run, on a tile of zoom `zoom`.
    #[must_use]
    pub fn new(ops: &'a [TileOpDef], zoom: u8) -> Self {
        Self { ops, zoom }
    }

    /// The operations that run at this zoom, in order.
    fn active(self) -> impl Iterator<Item = &'a TileOp> {
        self.ops
            .iter()
            .filter(move |def| def.zooms.contains(&self.zoom))
            .map(|def| &def.op)
    }

    /// Runs the operations of this zoom on `features`, in draw order.
    pub(crate) fn apply(self, features: &mut Vec<TileFeature>) {
        for op in self.active() {
            match *op {
                TileOp::Limit { count } => limit::apply(count, features),
            }
        }
    }
}
