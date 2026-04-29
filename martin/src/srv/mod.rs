// utoipa's `#[utoipa::path(...)]` macro generates a sibling `__path_<fn>`
// struct that `#[derive(OpenApi)]` resolves at the same import path as the
// handler. To keep the route modules themselves private (so clippy::pedantic
// doesn't see helpers like `DebouncedWarning::new`), we re-export each
// annotated handler and its `__path_*` sibling under the
// `unstable-schemas` feature gate, and `martin::schemas` addresses them via
// `crate::srv::*`.

#[cfg(feature = "fonts")]
mod fonts;
#[cfg(all(feature = "fonts", feature = "unstable-schemas"))]
pub use fonts::{__path_get_font, get_font};

mod server;
#[cfg(feature = "unstable-schemas")]
pub use server::{__path_get_health, get_health};
pub use server::{RESERVED_KEYWORDS, new_server, router};

mod admin;
pub use admin::Catalog;
#[cfg(feature = "unstable-schemas")]
pub use admin::{__path_get_catalog, get_catalog};

#[cfg(feature = "_tiles")]
mod tiles;
#[cfg(all(feature = "_tiles", feature = "unstable-schemas"))]
pub use tiles::content::{__path_get_tile, get_tile};
#[cfg(feature = "_tiles")]
pub use tiles::content::{DynTileSource, TileRequestHeaders};
#[cfg(feature = "_tiles")]
pub use tiles::metadata::merge_tilejson;
#[cfg(all(feature = "_tiles", feature = "unstable-schemas"))]
pub use tiles::metadata::{__path_get_source_info, get_source_info};

#[cfg(feature = "sprites")]
mod sprites;
#[cfg(all(feature = "sprites", feature = "unstable-schemas"))]
pub use sprites::{
    __path_get_sprite_json, __path_get_sprite_png, __path_get_sprite_sdf_json,
    __path_get_sprite_sdf_png, get_sprite_json, get_sprite_png, get_sprite_sdf_json,
    get_sprite_sdf_png,
};

#[cfg(feature = "styles")]
mod styles;
#[cfg(all(feature = "styles", feature = "unstable-schemas"))]
pub use styles::{__path_get_style_json, get_style_json};

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod styles_rendering;
#[cfg(all(
    feature = "rendering",
    target_os = "linux",
    feature = "unstable-schemas"
))]
pub use styles_rendering::{__path_get_style_rendered, get_style_rendered};
