use martin_tile_utils::{Encoding, Format, TileCoord};

use crate::cache::{CacheKey, Cacheable, ResourceCache};
use crate::tiles::Tile;

/// Tile cache for storing rendered tile data, keyed by source ID, tile
/// coordinate, query string, `Accept`-driven output format, and the
/// `Accept-Encoding`-negotiated encoding.
pub type TileCache = ResourceCache<TileCacheKey, Tile>;

/// Optional wrapper for [`TileCache`].
pub type OptTileCache = Option<TileCache>;

/// Constant representing no tile cache configuration.
pub const NO_TILE_CACHE: OptTileCache = None;

/// Cache key for one tile entry.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub enum TileCacheKey {
    /// A particular request shapes the bytes that would be served
    Dynamic {
        /// Source the tile belongs to, or the comma-separated sources of a composite.
        source_id: Box<str>,
        /// Tile coordinate.
        xyz: TileCoord,
        /// Request query string, when the source consumes one.
        query: Option<Box<str>>,
        /// Format requested via the `Accept` header
        /// `None` if absent.
        format: Option<Format>,
        /// Encoding negotiated via the `Accept-Encoding` header.
        /// `None` for the tile as the pre-cache pipeline produced it.
        encoding: Option<Encoding>,
    },

    /// A tile exactly as its source produced it, before any post-cache processing.
    Static {
        /// Source the tile belongs to.
        source_id: Box<str>,
        /// Tile coordinate.
        xyz: TileCoord,
    },
}

impl TileCacheKey {
    /// Key for the bytes a particular request shape produces.
    #[must_use]
    pub fn new_request_dynamic(
        source_id: impl Into<Box<str>>,
        xyz: TileCoord,
        query: Option<Box<str>>,
        format: Option<Format>,
        encoding: Option<Encoding>,
    ) -> Self {
        Self::Dynamic {
            source_id: source_id.into(),
            xyz,
            query,
            format,
            encoding,
        }
    }

    /// Key for a source's own bytes, independent of any request shape.
    #[must_use]
    pub fn new_request_static(source_id: impl Into<Box<str>>, xyz: TileCoord) -> Self {
        Self::Static {
            source_id: source_id.into(),
            xyz,
        }
    }

    /// The source this entry belongs to.
    #[must_use]
    pub fn source_id(&self) -> &str {
        match self {
            Self::Dynamic { source_id, .. } | Self::Static { source_id, .. } => source_id,
        }
    }

    /// The coordinate this entry is for.
    pub const fn xyz(&self) -> TileCoord {
        match self {
            Self::Dynamic { xyz, .. } | Self::Static { xyz, .. } => *xyz,
        }
    }
}

impl CacheKey for TileCacheKey {
    const CACHE_NAME: &'static str = "tile";

    fn matches_source(&self, source_id: &str) -> bool {
        self.source_id().split(',').any(|s| s == source_id)
    }

    fn record_outcome(&self, hit: bool) {
        #[cfg(feature = "metrics")]
        crate::metrics::TILE_CACHE_REQUESTS_TOTAL
            .with_label_values(&[
                Self::CACHE_NAME,
                crate::cache::hit_miss_label(hit),
                crate::metrics::ZOOM_LABELS[self.xyz().z() as usize],
            ])
            .inc();
        #[allow(clippy::if_same_then_else)]
        if hit {
            hotpath::gauge!("tile_cache_hits").inc(1.0);
        } else {
            hotpath::gauge!("tile_cache_misses").inc(1.0);
        }
    }

    fn heap_size(&self) -> usize {
        match self {
            Self::Dynamic { source_id, query, .. } => source_id.len() + query.as_deref().map_or(0, str::len),
            Self::Static { source_id, .. } => source_id.len(),
        }
    }
}

impl Cacheable for Tile {
    fn heap_size(&self) -> usize {
        let etag = if self.etag.is_heap_allocated() {
            self.etag.capacity()
        } else {
            0
        };
        self.data.len() + etag
    }
}

#[cfg(test)]
mod tests {
    use compact_str::CompactString;
    use martin_tile_utils::TileInfo;

    use super::*;

    #[test]
    fn heap_size_ignores_inline_etag() {
        let tile = Tile::new_with_etag(
            vec![0; 7],
            TileInfo::new(Format::Mvt, Encoding::Uncompressed),
            "short".into(),
        );

        assert_eq!(tile.heap_size(), 7);
    }

    #[test]
    fn heap_size_counts_heap_allocated_etag() {
        let etag = CompactString::from("\"0123456789abcdef0123456789abcdef\"".repeat(4));
        let capacity = etag.capacity();
        let tile = Tile::new_with_etag(vec![0; 7], TileInfo::new(Format::Mvt, Encoding::Uncompressed), etag);

        assert_eq!(tile.heap_size(), 7 + capacity);
    }
}
