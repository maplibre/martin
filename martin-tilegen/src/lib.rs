#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod assemble;
mod error;
pub mod pipeline;
pub mod props;
pub mod record;
mod sink;
mod sort;
mod sort_key;
mod tile;

pub use assemble::{LayerAssembler, LayerGrid};
pub use error::{TileGenError, TileGenResult};
#[cfg(feature = "mbtiles")]
pub use sink::MbtilesSink;
pub use sink::{EncodedTile, TileSink};
pub use sort::{Merger, SortBuffer, SortConfig, Sorter};
pub use sort_key::{LayerId, Seq, SortKey};
pub use tile::{MAX_ZOOM, TileId, TileOrder};
