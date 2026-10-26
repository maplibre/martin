//! [OGC API - Tiles](https://docs.ogc.org/is/20-057/20-057.html) under `/ogc`.

mod collections;
mod conformance;
mod data_type;
mod error;
#[cfg(all(feature = "rendering", target_os = "linux"))]
mod generated_style;
mod landing;
mod link;
#[cfg(all(feature = "rendering", target_os = "linux"))]
mod map;
#[cfg(all(feature = "rendering", target_os = "linux"))]
mod map_tiles;
mod openapi;
#[cfg(feature = "resources")]
mod styles;
mod tile;
mod tile_matrix_sets;
mod tilesets;

pub use collections::{get_collection, get_collections};
pub use conformance::get_conformance;
pub use landing::get_landing_page;
#[cfg(all(feature = "rendering", target_os = "linux", feature = "unstable-schemas"))]
pub use map::{__path_get_collection_map, __path_get_dataset_map, __path_get_style_map};
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use map::{get_collection_map, get_dataset_map, get_style_map};
#[cfg(all(feature = "rendering", target_os = "linux", feature = "unstable-schemas"))]
pub use map_tiles::{
    __path_get_dataset_map_tile, __path_get_dataset_map_tileset, __path_get_dataset_map_tilesets,
    __path_get_style_map_tile, __path_get_style_map_tileset, __path_get_style_map_tilesets,
};
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use map_tiles::{
    get_dataset_map_tile, get_dataset_map_tileset, get_dataset_map_tilesets, get_style_map_tile, get_style_map_tileset,
    get_style_map_tilesets,
};
pub use openapi::get_api;
#[cfg(all(feature = "resources", feature = "unstable-schemas"))]
pub use styles::__path_get_styles;
#[cfg(feature = "resources")]
pub use styles::get_styles;
pub use tile::get_tile;
pub use tile_matrix_sets::{get_tile_matrix_set, get_tile_matrix_sets};
pub use tilesets::{get_tileset, get_tilesets};
#[cfg(feature = "unstable-schemas")]
pub use {
    collections::{__path_get_collection, __path_get_collections},
    conformance::__path_get_conformance,
    landing::__path_get_landing_page,
    openapi::__path_get_api,
    tile::__path_get_tile,
    tile_matrix_sets::{__path_get_tile_matrix_set, __path_get_tile_matrix_sets},
    tilesets::{__path_get_tileset, __path_get_tilesets},
};
