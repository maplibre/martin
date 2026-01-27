#[cfg(feature = "fonts")]
mod fonts;

mod server;
pub use server::{RESERVED_KEYWORDS, new_server, router};

mod admin;
pub use admin::Catalog;

#[cfg(feature = "_tiles")]
mod tiles;
#[cfg(feature = "_tiles")]
pub use tiles::content::DynTileSource;
#[cfg(feature = "_tiles")]
pub use tiles::metadata::merge_tilejson;

#[cfg(feature = "sprites")]
mod sprites;

#[cfg(feature = "styles")]
mod styles;

#[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
mod styles_rendering;

mod redirects;
