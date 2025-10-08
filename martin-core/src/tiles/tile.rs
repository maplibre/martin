use martin_tile_utils::{TileData, TileInfo};

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
    /// Pre-computed etag/hash for the tile data (empty for empty tiles)
    pub etag: String,
}

impl Tile {
    /// Creates a new tile with the given tile data and metadata.
    ///
    /// For empty tiles, etag will be "0", otherwise [`xxh3_128(data)`](xxhash_rust::xxh3::xxh3_128).
    #[must_use]
    pub fn new(data: TileData, info: TileInfo) -> Self {
        let etag = if data.is_empty() {
            0
        } else {
            xxhash_rust::xxh3::xxh3_128(&data)
        };
        Self::new_with_etag(data, info, etag.to_string())
    }

    /// Creates a new tile with the given tile data, metadata, and etag.
    #[must_use]
    pub fn new_with_etag(data: TileData, info: TileInfo, etag: String) -> Self {
        Self { data, info, etag }
    }

    /// Returns true if the tile data is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}
