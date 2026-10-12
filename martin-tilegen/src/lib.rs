#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod assemble;
mod encode;
mod error;
mod group;
pub mod pipeline;
pub mod project;
pub mod props;
pub mod record;
mod render;
mod sink;
mod sort;
mod sort_key;
mod tile;

pub use assemble::{LayerAssembler, LayerGrid};
pub use encode::{DedupIndex, EncodeSettings, LayerInfo, LayerStats, TileEncoder, TileFormat};
pub use error::{TileGenError, TileGenResult};
pub use group::{LayerRecordRef, TileGrouper, TileRecords};
pub use render::{
    Feature, FeatureGeom, GeometryError, PixelThreshold, RenderLayer, RenderLayerError, Renderer,
    SliceError, SliceErrors,
};
#[cfg(feature = "mbtiles")]
pub use sink::MbtilesSink;
pub use sink::{EncodedTile, TileSink};
pub use sort::{Merger, SortBuffer, SortConfig, Sorter};
pub use sort_key::{LayerId, Seq, SortKey};
pub use tile::{MAX_ZOOM, TileId, TileOrder};
