#![doc = include_str!("README.md")]

mod errors;
mod mosaic;
mod source;
mod stac;
mod tile_index;

pub use crate::tiles::cog::errors::CogError;
pub use crate::tiles::cog::mosaic::CogMosaicOptions;
pub use crate::tiles::cog::source::CogSource;
pub use crate::tiles::cog::stac::{CogStacOptions, HrefResolver, StacBand, StacIndex, StacOrder, StacSearchOptions};
pub use crate::tiles::cog::tile_index::{CogTileIndexOptions, DEFAULT_LOCATION_FIELD, TileIndex};
