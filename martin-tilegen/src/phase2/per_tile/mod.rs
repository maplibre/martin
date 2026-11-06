//! Per-tile operations: each takes one layer's features in one tile, in draw order, and returns the new list.

mod limit;

use std::ops::Range;

use crate::plan::{TileOp, TileOpDef};

/// One feature of a layer in a tile, as ranges into the records and properties the assembler decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TileFeature {
    /// The feature's pieces in this tile.
    pub(crate) records: Range<usize>,
    /// The feature's properties.
    pub(crate) props: Range<usize>,
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
