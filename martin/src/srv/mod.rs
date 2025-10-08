#[cfg(feature = "fonts")]
mod fonts;

mod server;
pub use server::{Catalog, RESERVED_KEYWORDS, new_server, router};

#[cfg(feature = "_tiles")]
mod tiles;
#[cfg(feature = "_tiles")]
pub use tiles::{DynTileSource, TileRequest};

#[cfg(feature = "_tiles")]
mod tiles_info;
#[cfg(any(feature = "_tiles"))]
pub use tiles_info::SourceIDsRequest;
#[cfg(feature = "_tiles")]
pub use tiles_info::merge_tilejson;

#[cfg(feature = "sprites")]
mod sprites;

#[cfg(feature = "styles")]
mod styles;
