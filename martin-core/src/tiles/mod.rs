//! Tile management and representation for Martin tile server.
//!
//! This module provides core abstractions for working with map tiles.
//! We split this into two parts:
//! - a public facing catalog for exposing which tile sources exis
//! - the sources for accessing tiles

use std::collections::HashMap;

use martin_tile_utils::{TileData, TileInfo};

/// The public facing API for managing a catalog of tile sources
pub mod catalog;
mod source;
pub use source::{BoxedSource, Source};

/// URL query parameters for dynamic tile generation.
pub type UrlQuery = HashMap<String, String>;

mod error;
pub use error::*;

/// Represents a single map tile with its raw data and metadata.
///
/// Combines tile data (as raw bytes) with format and encoding information.
/// This is the fundamental unit that flows through the Martin tile server.
///
/// # Examples
///
/// ```rust
/// use martin_core::tiles::Tile;
/// use martin_tile_utils::{TileInfo, Format, Encoding};
///
/// let data = vec![0x89, 0x50, 0x4E, 0x47]; // PNG header
/// let info = TileInfo::new(Format::Png, Encoding::Uncompressed);
/// let tile = Tile::new(data, info);
/// ```
#[derive(Debug, Clone)]
pub struct Tile {
    /// Raw tile data as bytes (PNG, MVT, etc.)
    pub data: TileData,
    /// Metadata about the tile's format and encoding
    pub info: TileInfo,
}

impl Tile {
    /// Creates a new tile with the given tile data and metadata.
    #[must_use]
    pub fn new(data: TileData, info: TileInfo) -> Self {
        Self { data, info }
    }
}
