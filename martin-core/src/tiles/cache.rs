use martin_tile_utils::TileCoord;
use moka::future::Cache;
use moka::ops::compute::{CompResult, Op};
use std::sync::Mutex;
use tracing::{info, trace};

use crate::tiles::Tile;

/// Tile cache for storing rendered tile data.
#[derive(Clone, Debug)]
pub struct TileCache(Cache<TileCacheKey, Tile>);

impl TileCache {
    /// Creates a new tile cache with the specified maximum size in bytes.
    #[must_use]
    pub fn new(max_size_bytes: u64) -> Self {
        Self(
            Cache::builder()
                .name("tile_cache")
                .weigher(|_key: &TileCacheKey, value: &Tile| -> u32 {
                    value.data.len().try_into().unwrap_or(u32::MAX)
                })
                .max_capacity(max_size_bytes)
                .support_invalidation_closures()
                .build(),
        )
    }

    /// Gets a tile from cache or computes it using the provided function.
    pub async fn get_or_insert<F, Fut, E>(
        &self,
        source_id: String,
        xyz: TileCoord,
        query: Option<String>,
        compute: F,
    ) -> Result<Tile, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Tile, E>>,
    {
        let key = TileCacheKey::new(source_id, xyz, query);
        let error = Mutex::new(None);
        let result = self
            .0
            .entry(key.clone())
            .and_compute_with(|maybe_entry| {
                let error = &error;
                async move {
                    if maybe_entry.is_some() {
                        Op::Nop
                    } else {
                        match compute().await {
                            Ok(tile) => Op::Put(tile),
                            Err(err) => {
                                *error.lock().expect(
                                    "tile cache compute error mutex should not be poisoned",
                                ) = Some(err);
                                Op::Nop
                            }
                        }
                    }
                }
            })
            .await;

        if let Some(err) = error
            .into_inner()
            .expect("tile cache compute error mutex should not be poisoned")
        {
            return Err(err);
        }

        let (tile, is_hit) = match result {
            CompResult::Inserted(entry) | CompResult::ReplacedWith(entry) => {
                (entry.into_value(), false)
            }
            CompResult::Unchanged(entry) => (entry.into_value(), true),
            CompResult::Removed(_) | CompResult::StillNone(_) => {
                unreachable!("tile cache entry compute should not remove or remain empty")
            }
        };

        if is_hit {
            hotpath::gauge!("tile_cache_hits").inc(1.0);
            trace!(
                "Tile cache HIT for {key:?} (entries={entries}, size={size}B)",
                entries = self.0.entry_count(),
                size = self.0.weighted_size()
            );
        } else {
            hotpath::gauge!("tile_cache_misses").inc(1.0);
            trace!("Tile cache MISS for {key:?}");
        }

        Ok(tile)
    }

    /// Invalidates all cached tiles for a specific source.
    pub fn invalidate_source(&self, source_id: &str) {
        let source_id_owned = source_id.to_string();
        self.0
            .invalidate_entries_if(move |key, _| key.source_id == source_id_owned)
            .expect("invalidate_entries_if predicate should not error");
        info!("Invalidated tile cache for source: {source_id}");
    }

    /// Invalidates all cached tiles.
    pub fn invalidate_all(&self) {
        self.0.invalidate_all();
        info!("Invalidated all tile cache entries");
    }

    /// Returns the number of cached entries.
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        self.0.entry_count()
    }

    /// Returns the total size of cached data in bytes.
    #[must_use]
    pub fn weighted_size(&self) -> u64 {
        self.0.weighted_size()
    }

    /// Runs pending maintenance tasks (e.g. processing invalidation predicates).
    pub async fn run_pending_tasks(&self) {
        self.0.run_pending_tasks().await;
    }
}

/// Optional wrapper for `TileCache`.
pub type OptTileCache = Option<TileCache>;

/// Constant representing no tile cache configuration.
pub const NO_TILE_CACHE: OptTileCache = None;

/// Cache key for tile data.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct TileCacheKey {
    source_id: String,
    xyz: TileCoord,
    query: Option<String>,
}

impl TileCacheKey {
    fn new(source_id: String, xyz: TileCoord, query: Option<String>) -> Self {
        Self {
            source_id,
            xyz,
            query,
        }
    }
}
