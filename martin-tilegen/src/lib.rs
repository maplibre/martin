#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod assemble;
mod block;
mod columns;
mod encode;
mod error;
pub mod expr;
mod generate;
mod group;
mod pipeline;
pub mod plan;
pub mod project;
pub mod props;
pub(crate) mod record;
mod render;
mod simplify;
mod sink;
mod sort;
mod sort_key;
pub mod source;
mod tile;
mod tile_render;

pub(crate) use assemble::LayerAssembler;
pub use assemble::LayerGrid;
pub use block::render_block;
pub use encode::TileFormat;
pub(crate) use encode::{DedupIndex, EncodeSettings, LayerInfo, LayerStats, TileEncoder};
pub use error::{TileGenError, TileGenResult};
pub use generate::{GenerateConfig, LayerSliceErrors, Progress, Summary, generate};
pub(crate) use group::{TileGrouper, TileRecords};
pub use map_tile_toolkit::ClipMode;
pub use plan::ExprErrors;
pub use render::{
    Feature, FeatureGeom, GeometryError, PixelThreshold, RenderLayer, RenderLayerError,
};
pub(crate) use render::{Renderer, SliceErrors};
#[cfg(feature = "mbtiles")]
pub use sink::MbtilesSink;
#[cfg(feature = "pmtiles")]
pub use sink::PmtilesSink;
pub use sink::{EncodedTile, TileSink};
pub use sort::SortConfig;
pub(crate) use sort::{Merger, SortBuffer, Sorter};
pub(crate) use sort_key::{LayerId, Seq, SortKey};
pub use tile::{MAX_ZOOM, TileId, TileOrder};

/// The stages a render runs through, for this crate's benches and stage-level tests only.
#[cfg(feature = "internals")]
#[doc(hidden)]
pub mod internals {
    pub use crate::assemble::{AssembledLayer, LayerAssembler};
    pub use crate::encode::{DedupIndex, EncodeSettings, LayerInfo, LayerStats, TileEncoder};
    pub use crate::group::{LayerRecordRef, TileGrouper, TileRecords};
    pub use crate::render::{Renderer, SliceError, SliceErrors};
    pub use crate::sort::{Merger, SortBuffer, Sorter};
    pub use crate::sort_key::{LayerId, Seq, SortKey};

    /// The encoded form of a rendered piece, as it is sorted and grouped.
    pub mod record {
        pub use crate::record::*;
    }
}
