//! [OGC API - Tiles](https://docs.ogc.org/is/20-057/20-057.html) under `/ogc`.

mod collections;
mod conformance;
mod data_type;
mod error;
mod landing;
mod link;
mod tile;
mod tile_matrix_sets;
mod tilesets;

pub use collections::{get_collection, get_collections};
pub use conformance::get_conformance;
pub use landing::get_landing_page;
pub use tile::get_tile;
pub use tile_matrix_sets::{get_tile_matrix_set, get_tile_matrix_sets};
pub use tilesets::{get_tileset, get_tilesets};
