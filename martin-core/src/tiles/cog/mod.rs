#![doc = include_str!("README.md")]

mod errors;
mod mosaic;
mod source;
mod stac;
mod tile_index;

pub use crate::tiles::cog::errors::CogError;
pub use crate::tiles::cog::mosaic::CogMosaicOptions;
pub use crate::tiles::cog::source::CogSource;
pub use crate::tiles::cog::stac::CogStacOptions;
pub use crate::tiles::cog::tile_index::CogTileIndexOptions;
pub use cog_tiles::{
    DEFAULT_LOCATION_FIELD, DEFAULT_MAX_CACHED_SEARCHES, DEFAULT_MAX_ITEMS_PER_SEARCH, DEFAULT_SEARCH_EXPIRY,
    HrefResolver, StacBand, StacIndex, StacOrder, StacSearchOptions, TileEncoding, TileIndex,
};
