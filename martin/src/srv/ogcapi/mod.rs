//! [OGC API - Tiles](https://docs.ogc.org/is/20-057/20-057.html) under `/ogc`.

mod collections;
mod conformance;
mod data_type;
mod error;
mod landing;
mod link;
#[cfg(feature = "resources")]
mod styles;
mod tile;
mod tile_matrix_sets;
mod tilesets;

pub use collections::{get_collection, get_collections};
pub use conformance::get_conformance;
pub use landing::get_landing_page;
#[cfg(all(feature = "resources", feature = "unstable-schemas"))]
pub use styles::{__path_get_style, __path_get_styles};
#[cfg(feature = "resources")]
pub use styles::{get_style, get_styles};
pub use tile::get_tile;
pub use tile_matrix_sets::{get_tile_matrix_set, get_tile_matrix_sets};
pub use tilesets::{get_tileset, get_tilesets};
#[cfg(feature = "unstable-schemas")]
pub use {
    collections::{__path_get_collection, __path_get_collections},
    conformance::__path_get_conformance,
    landing::__path_get_landing_page,
    tile::__path_get_tile,
    tile_matrix_sets::{__path_get_tile_matrix_set, __path_get_tile_matrix_sets},
    tilesets::{__path_get_tileset, __path_get_tilesets},
};
