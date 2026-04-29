#[cfg(feature = "fonts")]
mod fonts;

mod server;
pub use server::{RESERVED_KEYWORDS, new_server, router};
// utoipa's `#[utoipa::path(...)]` macro generates a sibling `__path_<fn>`
// struct that `#[derive(OpenApi)]` resolves at the same import path as the
// handler. Re-export both so `martin::schemas` can address them via
// `crate::srv::get_health` without making `mod server` itself `pub` (that
// would expose private helpers to clippy::pedantic / `must_use_candidate`).
#[cfg(feature = "unstable-schemas")]
pub use server::{__path_get_health, get_health};

mod admin;
pub use admin::Catalog;
#[cfg(feature = "unstable-schemas")]
pub use admin::{__path_get_catalog, get_catalog};

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
