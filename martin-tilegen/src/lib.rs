#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod assemble;
mod encode;
mod error;
pub mod expr;
mod generate;
mod group;
pub mod pipeline;
pub mod plan;
pub mod project;
mod props;
pub mod record;
mod render;
mod sink;
mod sort;
mod sort_key;
mod tile;

pub use assemble::{AssembledLayer, LayerAssembler, LayerGrid};
pub use encode::{DedupIndex, EncodeSettings, LayerInfo, LayerStats, TileEncoder, TileFormat};
pub use error::{TileGenError, TileGenResult};
pub use generate::{ExprErrors, GenerateConfig, LayerSliceErrors, Progress, Summary, generate};
pub use group::{LayerRecordRef, TileGrouper, TileRecords};
pub use map_tile_toolkit::ClipMode;
pub use render::{
    Feature, FeatureGeom, GeometryError, PixelThreshold, RenderLayer, RenderLayerError, Renderer,
    SliceError, SliceErrors,
};
#[cfg(feature = "mbtiles")]
pub use sink::MbtilesSink;
#[cfg(feature = "pmtiles")]
pub use sink::PmtilesSink;
pub use sink::{EncodedTile, TileSink};
pub use sort::{Merger, SortBuffer, SortConfig, Sorter};
pub use sort_key::{LayerId, Seq, SortKey};
pub use tile::{MAX_ZOOM, TileId, TileOrder};
