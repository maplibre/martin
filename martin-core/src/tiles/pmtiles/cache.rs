use std::time::Duration;

use moka::future::Cache;
use tracing::info;

/// Optional wrapper for `PmtCache`.
pub type OptPmtCache = Option<PmtCache>;

/// Constant representing no `PMTiles` cache configuration.
pub const NO_PMT_CACHE: OptPmtCache = None;

/// Globally shared `PMTiles` directory cache for storing `PMTiles` directory structures.
///
/// For access to the cache, use the [`PmtCacheInstance`] struct instead, as this way the cache can have a consistent view into how large it is.
#[derive(Clone, Debug)]
pub struct PmtCache(Cache<PmtCacheKey, pmtiles::Directory>);

impl PmtCache {
    /// Creates a new `PMTiles` directory cache instance
    ///
    /// # Arguments
    ///
    /// * `max_size_bytes` - Maximum cache size in bytes (based on directory data size)
    /// * `expiry` - Optional maximum lifetime (TTL - time to live from creation)
    /// * `idle_timeout` - Optional idle timeout (TTI - time to idle since last access)
    #[must_use]
    pub fn new(
        max_size_bytes: u64,
        expiry: Option<Duration>,
        idle_timeout: Option<Duration>,
    ) -> Self {
        let mut builder = Cache::builder()
            .name("pmtiles_directory_cache")
            .weigher(|_key: &PmtCacheKey, value: &pmtiles::Directory| -> u32 {
                value.get_approx_byte_size().try_into().unwrap_or(u32::MAX)
                    + size_of::<PmtCacheKey>().try_into().unwrap_or(u32::MAX)
            })
            .max_capacity(max_size_bytes);

        if let Some(ttl) = expiry {
            builder = builder.time_to_live(ttl);
        }

        if let Some(tti) = idle_timeout {
            builder = builder.time_to_idle(tti);
        }

        Self(builder.build())
    }
}

impl Default for PmtCache {
    fn default() -> Self {
        Self::new(0, None, None)
    }
}

/// `PMTiles` directory cache for storing `PMTiles` directory structures.
#[derive(Clone, Debug)]
pub struct PmtCacheInstance {
    /// Unique identifier for this cache instance
    ///
    /// We need this as we want to share the cache (and thus the cache size) across multiple sources.
    id: usize,
    /// Cache storing (id, offset) -> `pmtiles::Directory`
    cache: PmtCache,
}

impl PmtCacheInstance {
    /// Creates a new `PMTiles` directory cache instance
    #[must_use]
    pub fn new(id: usize, cache: PmtCache) -> Self {
        Self { id, cache }
    }

    /// Returns the cache ID.
    #[must_use]
    pub fn id(&self) -> usize {
        self.id
    }

    /// Invalidates all cached directories for this `PMTiles` file.
    pub fn invalidate_all(&self) {
        self.cache.0.invalidate_all();
        info!("Invalidated PMTiles directory cache for id={}", self.id);
    }

    /// Syncs pending operations to make cache statistics consistent.
    ///
    /// This forces the cache to apply pending operations immediately,
    /// ensuring that `entry_count()` and `weighted_size()` return accurate values.
    pub async fn sync(&self) {
        self.cache.0.run_pending_tasks().await;
    }

    /// Returns the number of cached entries.
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        self.cache.0.entry_count()
    }

    /// Returns the total size of cached data in bytes.
    #[must_use]
    pub fn weighted_size(&self) -> u64 {
        self.cache.0.weighted_size()
    }
}

impl pmtiles::DirectoryCache for PmtCacheInstance {
    async fn get_dir_entry_or_insert(
        &self,
        offset: usize,
        tile_id: pmtiles::TileId,
        fetcher: impl Future<Output = pmtiles::PmtResult<pmtiles::Directory>> + Send,
    ) -> pmtiles::PmtResult<Option<pmtiles::DirEntry>> {
        let key = PmtCacheKey::new(self.id, offset);
        let directory = self.cache.0.try_get_with(key, fetcher).await.map_err(|e| {
            pmtiles::PmtError::DirectoryCacheError(format!("Moka cache fetch error: {e}"))
        })?;
        Ok(directory.find_tile_id(tile_id).cloned())
    }
}

/// Cache key for `PMTiles` directory data.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct PmtCacheKey {
    id: usize,
    offset: usize,
}

impl PmtCacheKey {
    fn new(id: usize, offset: usize) -> Self {
        Self { id, offset }
    }
}
