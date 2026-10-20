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

#[cfg(feature = "object-store")]
pub use object_store;

#[cfg(feature = "fs")]
pub use crate::backend::FileBackend;
#[cfg(feature = "object-store")]
pub use crate::backend::ObjectStoreBackend;
pub use crate::backend::{AsyncBackend, MemoryBackend};
pub use crate::error::{CogError, CogResult, SkippedImage, UnsupportedReason};
pub use crate::reader::{AsyncCogReader, CogInfo, DEFAULT_PREFETCH, TileFormat};
pub use crate::tile::{
    EARTH_CIRCUMFERENCE, HALF_EARTH, MAX_ZOOM, TileCoord, mercator_to_lnglat, resolution, tile_extent,
};
