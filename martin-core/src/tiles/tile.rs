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
        let etag = hash_etag(&data);
        Self { data, info, etag }
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

/// Hashes tile data into a base64 etag.
///
/// For empty data the hash is `0`, otherwise [`xxh3_128`](xxhash_rust::xxh3::xxh3_128) of the data.
fn hash_etag(data: &[u8]) -> String {
    let hash = if data.is_empty() {
        0
    } else {
        xxhash_rust::xxh3::xxh3_128(data)
    };
    URL_SAFE_NO_PAD.encode(hash.to_ne_bytes())
}

#[cfg(test)]
mod tests {
    use martin_tile_utils::{Encoding, Format, TileInfo};

    use super::*;

    fn is_valid_entity_tag(tag: &str) -> bool {
        !tag.is_empty()
            && tag
                .bytes()
                .all(|c| c == 0x21 || (0x23..=0x7E).contains(&c) || c >= 0x80)
    }
    fn info() -> TileInfo {
        TileInfo::new(Format::Mvt, Encoding::Uncompressed)
    }

    #[test]
    fn hashed_etag_is_used_as_is() {
        let tile = Tile::new_hash_etag(vec![1, 2, 3], info());
        assert_eq!(tile.strong_etag(), tile.etag);
        assert!(is_valid_entity_tag(&tile.strong_etag()));
    }

    #[test]
    fn valid_upstream_etag_is_preserved() {
        let tile = Tile::new_with_etag(vec![1, 2, 3], info(), "abc123".to_string());
        assert_eq!(tile.strong_etag(), "abc123");
    }

    #[test]
    fn invalid_upstream_etag_falls_back_to_hash() {
        let data = vec![1, 2, 3];
        // A quoted upstream etag would make `EntityTag::new_strong` panic.
        let tile = Tile::new_with_etag(data.clone(), info(), "\"abc\"".to_string());
        let etag = tile.strong_etag();
        assert!(is_valid_entity_tag(&etag));
        assert_eq!(etag, hash_etag(&data));
    }

    #[test]
    fn empty_upstream_etag_falls_back_to_hash() {
        let tile = Tile::new_with_etag(vec![1, 2, 3], info(), String::new());
        assert!(is_valid_entity_tag(&tile.strong_etag()));
    }
}
