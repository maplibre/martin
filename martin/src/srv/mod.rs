#[cfg(feature = "fonts")]
mod fonts;

// `pub mod` so the `unstable-schemas` feature can name `get_health` and
// `get_catalog` from `martin::schemas` for utoipa's `#[openapi(paths(...))]`.
pub mod server;
pub use server::{RESERVED_KEYWORDS, new_server, router};

pub mod admin;
pub use admin::Catalog;

#[cfg(feature = "_tiles")]
mod tiles;
#[cfg(feature = "_tiles")]
pub use tiles::content::{DynTileSource, TileRequestHeaders};
#[cfg(feature = "_tiles")]
pub use tiles::metadata::merge_tilejson;

#[cfg(feature = "sprites")]
mod sprites;

#[cfg(feature = "styles")]
mod styles;

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod styles_rendering;
