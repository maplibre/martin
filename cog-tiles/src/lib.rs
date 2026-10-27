#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod backend;
mod bands;
#[cfg(feature = "mosaic")]
mod crs;
#[cfg(feature = "png")]
mod encode;
mod error;
mod geo;
#[cfg(feature = "mosaic")]
mod indexed;
mod jpeg;
mod metadata;
#[cfg(feature = "mosaic")]
mod mosaic;
mod pixels;
mod reader;
mod template;
mod tile;
#[cfg(feature = "mosaic")]
mod warp;

#[cfg(feature = "object-store")]
pub use object_store;

#[cfg(feature = "fs")]
pub use crate::backend::FileBackend;
#[cfg(feature = "object-store")]
pub use crate::backend::ObjectStoreBackend;
pub use crate::backend::{AsyncBackend, MemoryBackend};
#[cfg(feature = "png")]
pub use crate::encode::TileEncoding;
pub use crate::error::{Chunking, CogError, CogResult, SkippedImage, UnsupportedReason};
#[cfg(feature = "mosaic")]
pub use crate::indexed::{
    AsyncIndexedMosaic, INDEXED_TILE_SIZE, IndexedMosaicInfo, IndexedMosaicOptions, MosaicFile, MosaicIndex,
};
#[cfg(feature = "mosaic")]
pub use crate::mosaic::{
    AsyncCogMosaic, DEFAULT_FILE_EXPIRY, DEFAULT_MAX_CACHED_FILES, MosaicFileError, MosaicInfo, MosaicOptions,
};
pub use crate::pixels::{PixelWindow, TilePixels};
pub use crate::reader::{AsyncCogReader, CogInfo, DEFAULT_PREFETCH, TileFormat};
pub use crate::template::{KeyTemplate, MosaicTemplateError};
pub use crate::tile::{
    EARTH_CIRCUMFERENCE, HALF_EARTH, MAX_LATITUDE, MAX_ZOOM, TileCoord, lnglat_to_mercator, mercator_to_lnglat,
    resolution, tile_extent,
};
