use martin_tile_utils::{Format, TileCoord};

use crate::cache::{CacheKey, Cacheable, ResourceCache};
use crate::tiles::Tile;

/// Tile cache for storing rendered tile data, keyed by source ID, tile
/// coordinate, query string, and `Accept`-driven output format.
pub type TileCache = ResourceCache<TileCacheKey, Tile>;

/// Optional wrapper for [`TileCache`].
pub type OptTileCache = Option<TileCache>;

/// Constant representing no tile cache configuration.
pub const NO_TILE_CACHE: OptTileCache = None;

/// Cache key for a rendered tile.
///
/// Source-based invalidation matches exactly on `source_id` (each tile
/// belongs to one source). Metric recording adds a `zoom` dimension on top
/// of the standard cache/hit labels.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct TileCacheKey {
    source_id: String,
    xyz: TileCoord,
    query: Option<String>,
    /// Format requested via the `Accept` header; `None` if absent.
    format: Option<Format>,
}

impl TileCacheKey {
    /// Build a key from the request fields.
    #[must_use]
    pub fn new(
        source_id: String,
        xyz: TileCoord,
        query: Option<String>,
        format: Option<Format>,
    ) -> Self {
        Self {
            source_id,
            xyz,
            query,
            format,
        }
    }
}

impl CacheKey for TileCacheKey {
    const CACHE_NAME: &'static str = "tile";

    fn matches_source(&self, source_id: &str) -> bool {
        self.source_id == source_id
    }

    fn record_outcome(&self, hit: bool) {
        #[cfg(feature = "metrics")]
        crate::metrics::TILE_CACHE_REQUESTS_TOTAL
            .with_label_values(&[
                Self::CACHE_NAME,
                crate::cache::hit_miss_label(hit),
                crate::metrics::ZOOM_LABELS[self.xyz.z as usize],
            ])
            .inc();
        #[allow(
            clippy::if_same_then_else,
            reason = "hotpath::gauge! requires a literal name argument"
        )]
        if hit {
            hotpath::gauge!("tile_cache_hits").inc(1.0);
        } else {
            hotpath::gauge!("tile_cache_misses").inc(1.0);
        }
    }
}

impl Cacheable for Tile {
    fn weight(&self) -> u32 {
        self.data.len().try_into().unwrap_or(u32::MAX)
    }
}
