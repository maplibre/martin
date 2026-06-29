use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
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
/// let tile = Tile::new_hash_etag(data, info);
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
    /// For empty tiles, etag will be base64 of `0`, otherwise base64 of [`xxh3_128(data)`](xxhash_rust::xxh3::xxh3_128).
    #[must_use]
    pub fn new_hash_etag(data: TileData, info: TileInfo) -> Self {
        let etag = if data.is_empty() {
            0
        } else {
            xxhash_rust::xxh3::xxh3_128(&data)
        };
        let etag_base64 = URL_SAFE_NO_PAD.encode(etag.to_ne_bytes());
        Self {
            data,
            info,
            etag: etag_base64,
        }
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

/// Whether every byte of `tag` is allowed in an HTTP entity-tag body (RFC 9110 `etagc`).
fn is_valid_etag(tag: &str) -> bool {
    tag.bytes().all(|b| b == 0x21 || (0x23..=0x7e).contains(&b))
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::{Encoding, Format};

    use super::*;

    fn info() -> TileInfo {
        TileInfo::new(Format::Mvt, Encoding::Uncompressed)
    }

    #[test]
    fn strong_etag_reuses_valid_tag() {
        let tile = Tile::new_with_etag(b"data".to_vec(), info(), "upstream-tag".to_string());
        assert_eq!(tile.strong_etag(), "upstream-tag");
    }

    #[test]
    fn strong_etag_hashes_invalid_tag() {
        // A `"` is not a valid entity-tag character and would panic `EntityTag::new_strong`.
        let tile = Tile::new_with_etag(b"data".to_vec(), info(), "bad\"tag".to_string());
        let etag = tile.strong_etag();
        assert_ne!(etag, "bad\"tag");
        assert!(is_valid_etag(&etag));
        // Deterministic content hash, matching what `new_hash_etag` produces for the same data.
        assert_eq!(etag, Tile::new_hash_etag(b"data".to_vec(), info()).etag);
    }
}
