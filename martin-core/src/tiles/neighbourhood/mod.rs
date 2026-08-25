//! Assembles a 3x3 tile neighbourhood into one RGBA field.
//! This is for a pass whose kernel reads past a tile's edge sees real neighbouring terrain to avoid seams.

mod assemble;
mod etag;

pub use assemble::{
    FIELD_SIDE, GRID_SIDE, NEIGHBOURHOOD_LEN, Neighbourhood, NeighbourhoodError, RgbaField,
    TILE_SIZE,
};
pub use etag::{InputEtag, neighbourhood_etag};
