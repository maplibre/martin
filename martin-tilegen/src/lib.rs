#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod assemble;
mod encode;
mod error;
mod group;
mod key;
pub mod pipeline;
pub mod project;
pub mod props;
pub mod record;
mod render;
mod sink;
mod sort;
mod tile;

pub use assemble::{LayerAssembler, LayerGrid};
pub use encode::{
    DedupIndex, EncodeSettings, FeatureOrder, LayerInfo, LayerStats, TileEncoder, TileFormat,
};
pub use error::{TileGenError, TileGenResult};
pub use group::{TileGrouper, TileRecords};
pub use key::{LayerId, Seq, SortKey};
pub use render::{Feature, FeatureGeom, PixelThreshold, RenderLayer, Renderer};
#[cfg(feature = "mbtiles")]
pub use sink::MbtilesSink;
pub use sink::{EncodedTile, TileSink};
pub use sort::{Merger, SortBuffer, SortConfig, Sorter};
pub use tile::{MAX_ZOOM, TileId, TileOrder};
