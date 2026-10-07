#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod error;
mod key;
#[cfg(feature = "mbtiles")]
mod mbtiles_sink;
mod ordered;
mod sink;
mod sort;
mod tile;

pub use error::{TileGenError, TileGenResult};
pub use key::{Seq, SortKey};
#[cfg(feature = "mbtiles")]
pub use mbtiles_sink::MbtilesOutput;
pub use ordered::{OrderedBatches, OrderedWriter};
pub use sink::{DedupHint, EncodedTile, TileSink};
pub use sort::{Merger, SortBuffer, SortConfig, Sorter};
pub use tile::{MAX_ZOOM, TileOrder};
