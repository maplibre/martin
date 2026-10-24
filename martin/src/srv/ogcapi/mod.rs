//! [OGC API - Tiles](https://docs.ogc.org/is/20-057/20-057.html) under `/ogc`.

mod conformance;
mod error;
mod landing;
mod link;
mod tile_matrix_sets;

pub use conformance::get_conformance;
pub use landing::get_landing_page;
pub use tile_matrix_sets::{get_tile_matrix_set, get_tile_matrix_sets};
