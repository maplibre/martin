use martin_tile_utils::{Encoding, Format, PackedTileCoord, TileCoord};
use thin_vec::ThinVec;

use crate::SourceId;
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
///
/// 40 bytes: sources are interned [`SourceId`]s, so a lookup hashes no source names.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub enum TileCacheKey {
    /// A particular request shapes the bytes that would be served
    Dynamic {
        /// The source, or the server-defined set of sources (an alias) the tile combines.
        sources: SourceId,
        /// Tile coordinate.
        xyz: PackedTileCoord,
        /// Request query string, when the source consumes one.
        query: Option<Box<str>>,
        /// Format requested via the `Accept` header
        /// `None` if absent.
        format: Option<Format>,
        /// Encoding negotiated via the `Accept-Encoding` header.
        /// `None` for the tile as the pre-cache pipeline produced it.
        encoding: Option<Encoding>,
    },

    /// Like [`Self::Dynamic`], for a list of sources the client picked and no alias defines.
    AdHoc {
        /// The sources the tile combines, in request order.
        sources: ThinVec<SourceId>,
        /// Tile coordinate.
        xyz: PackedTileCoord,
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
        source: SourceId,
        /// Tile coordinate.
        xyz: PackedTileCoord,
    },
}

impl TileCacheKey {
    /// Key for the bytes a particular request shape produces.
    ///
    /// `sources` are combined in order; a list an alias defines shares the alias's entry.
    #[must_use]
    pub fn new_request_dynamic(
        sources: &[SourceId],
        xyz: TileCoord,
        query: Option<Box<str>>,
        format: Option<Format>,
        encoding: Option<Encoding>,
    ) -> Self {
        let xyz = xyz.pack();
        match SourceId::find_set(sources) {
            Some(sources) => Self::Dynamic {
                sources,
                xyz,
                query,
                format,
                encoding,
            },
            None => Self::AdHoc {
                sources: ThinVec::from(sources),
                xyz,
                query,
                format,
                encoding,
            },
        }
    }

    /// Key for a source's own bytes, independent of any request shape.
    #[must_use]
    pub fn new_request_static(source: SourceId, xyz: TileCoord) -> Self {
        Self::Static {
            source,
            xyz: xyz.pack(),
        }
    }

    /// The coordinate this entry is for.
    pub const fn xyz(&self) -> TileCoord {
        match self {
            Self::Dynamic { xyz, .. } | Self::AdHoc { xyz, .. } | Self::Static { xyz, .. } => xyz.unpack(),
        }
    }
}

impl CacheKey for TileCacheKey {
    const CACHE_NAME: &'static str = "tile";

    fn matches_source(&self, source: SourceId) -> bool {
        match self {
            Self::Dynamic { sources, .. } => sources.contains(source),
            Self::AdHoc { sources, .. } => sources.contains(&source),
            Self::Static { source: own, .. } => *own == source,
        }
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
            Self::Dynamic { query, .. } => query.as_deref().map_or(0, str::len),
            Self::AdHoc { sources, query, .. } => thin_vec_heap_size(sources) + query.as_deref().map_or(0, str::len),
            Self::Static { .. } => 0,
        }
    }
}

/// Bytes a [`ThinVec`] owns: its length and capacity header, then the items; nothing while empty.
fn thin_vec_heap_size<T>(items: &ThinVec<T>) -> usize {
    if items.capacity() == 0 {
        0
    } else {
        2 * size_of::<usize>() + items.capacity() * size_of::<T>()
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
    fn key_is_forty_bytes() {
        assert_eq!(size_of::<TileCacheKey>(), 40);
    }

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
