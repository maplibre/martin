#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod backend;
#[cfg(feature = "png")]
mod encode;
mod error;
mod geo;
mod jpeg;
mod reader;
mod tile;

#[cfg(feature = "fs")]
pub use backend::FileBackend;
#[cfg(feature = "object-store")]
pub use backend::ObjectStoreBackend;
pub use backend::{AsyncBackend, MemoryBackend};
pub use error::{CogError, CogResult, SkippedImage, UnsupportedReason};
#[cfg(feature = "object-store")]
pub use object_store;
pub use reader::{AsyncCogReader, CogInfo, DEFAULT_PREFETCH, TileFormat};
pub use tile::{
    EARTH_CIRCUMFERENCE, HALF_EARTH, MAX_ZOOM, TileCoord, mercator_to_lnglat, resolution,
    tile_extent,
};
