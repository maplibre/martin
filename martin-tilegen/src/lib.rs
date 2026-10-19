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

pub use map_tile_toolkit::ClipMode;

pub use crate::block::render_block;
pub use crate::error::{TileGenError, TileGenResult};
pub use crate::generate::{GenerateConfig, LayerSliceErrors, Progress, Summary, generate};
pub use crate::phase1::{
    Feature, FeatureGeom, GeometryError, PixelThreshold, RenderLayer, RenderLayerError, project,
};
pub use crate::phase2::TileFormat;
pub use crate::plan::{ExprErrors, LayerGrid};
#[cfg(feature = "mbtiles")]
pub use crate::sink::MbtilesSink;
#[cfg(feature = "pmtiles")]
pub use crate::sink::PmtilesSink;
pub use crate::sink::{EncodedTile, TileSink};
pub use crate::sort::SortConfig;
pub use crate::tile::{MAX_ZOOM, TileId, TileOrder};

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
