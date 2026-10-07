#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod error;
mod key;
pub mod pipeline;
mod sink;
mod sort;
mod tile;

pub use error::{TileGenError, TileGenResult};
pub use key::{LayerId, Seq, SortKey};
#[cfg(feature = "mbtiles")]
pub use sink::MbtilesSink;
pub use sink::{EncodedTile, TileSink};
pub use sort::{Merger, SortBuffer, SortConfig, Sorter};
pub use tile::{MAX_ZOOM, TileId, TileOrder};
