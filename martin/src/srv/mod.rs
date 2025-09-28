#[cfg(feature = "fonts")]
mod fonts;

mod server;
pub use server::{Catalog, RESERVED_KEYWORDS, new_server, router};

mod tiles;
pub use tiles::{DynTileSource, TileRequest};

mod tiles_info;
pub use tiles_info::{SourceIDsRequest, merge_tilejson};

#[cfg(feature = "sprites")]
mod sprites;

#[cfg(feature = "styles")]
mod styles;

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod styles_rendering;
