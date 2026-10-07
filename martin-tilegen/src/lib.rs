#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod error;
mod key;
mod tile;

pub use error::{TileGenError, TileGenResult};
pub use key::{Seq, SortKey};
pub use tile::{MAX_ZOOM, TileOrder};
