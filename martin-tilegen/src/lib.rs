#![doc = include_str!("../README.md")]
#![deny(unsafe_code)]

mod block;
mod error;
pub mod expr;
mod generate;
mod phase1;
mod phase2;
mod pipeline;
pub mod plan;
pub mod props;
mod sink;
mod sort;
pub mod source;
mod tile;

pub use block::render_block;
pub use error::{TileGenError, TileGenResult};
pub use generate::{GenerateConfig, LayerSliceErrors, Progress, Summary, generate};
pub use map_tile_toolkit::ClipMode;
pub use phase1::project;
pub use phase1::{
    Feature, FeatureGeom, GeometryError, PixelThreshold, RenderLayer, RenderLayerError,
};
pub use phase2::TileFormat;
pub use plan::{ExprErrors, LayerGrid};
#[cfg(feature = "mbtiles")]
pub use sink::MbtilesSink;
#[cfg(feature = "pmtiles")]
pub use sink::PmtilesSink;
pub use sink::{EncodedTile, TileSink};
pub use sort::SortConfig;
pub use tile::{MAX_ZOOM, TileId, TileOrder};

/// The stages a render runs through, for this crate's benches and stage-level tests only.
#[cfg(feature = "internals")]
#[doc(hidden)]
pub mod internals {
    pub use crate::phase1::internals::*;
    pub use crate::phase2::internals::*;
    pub use crate::sort::{
        LayerId, LayerRecordRef, Merger, Seq, SortBuffer, SortKey, Sorter, TileGrouper, TileRecords,
    };

    /// The encoded form of a rendered piece, as it is sorted and grouped.
    pub mod record {
        pub use crate::sort::record::*;
    }
}
