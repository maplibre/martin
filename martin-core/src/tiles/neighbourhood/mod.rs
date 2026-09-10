//! Assembles a 3x3 tile neighbourhood into one RGBA field.
//! This is for a pass whose kernel reads past a tile's edge sees real neighbouring terrain to avoid seams.

mod assemble;
mod etag;

#[cfg(feature = "hillshade")]
pub(crate) use assemble::CHANNELS;
pub use assemble::{
    DEFAULT_TILE_SIZE, GRID_SIDE, NEIGHBOURHOOD_LEN, Neighbourhood, NeighbourhoodError, RgbaField,
};
pub use etag::{InputEtag, neighbourhood_etag};
