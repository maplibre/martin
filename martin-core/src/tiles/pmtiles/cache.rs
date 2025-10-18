use moka::future::Cache;

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
    #[must_use]
    pub fn new(max_size_bytes: u64) -> Self {
        let cache = Cache::builder()
            .name("pmtiles_directory_cache")
            .weigher(|_key: &PmtCacheKey, value: &pmtiles::Directory| -> u32 {
                value.get_approx_byte_size().try_into().unwrap_or(u32::MAX)
                    + size_of::<PmtCacheKey>().try_into().unwrap_or(u32::MAX)
            })
            .max_capacity(max_size_bytes)
            .build();
        Self(cache)
    }
}

impl Default for PmtCache {
    fn default() -> Self {
        Self::new(0)
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

    /// Retrieves a directory from cache if present.
    async fn get(&self, offset: usize) -> Option<pmtiles::Directory> {
        let key = PmtCacheKey::new(self.id, offset);
        let result = self.cache.0.get(&key).await;

        if result.is_some() {
            log::trace!(
                "PMTiles directory cache HIT for id={id}, offset={offset} (entries={entries}, size={size})",
                id = self.id,
                entries = self.cache.0.entry_count(),
                size = self.cache.0.weighted_size()
            );
        } else {
            log::trace!(
                "PMTiles directory cache MISS for id={id}, offset={offset}",
                id = self.id,
            );
        }

        result
    }

    /// Returns the cache ID.
    #[must_use]
    pub fn id(&self) -> usize {
        self.id
    }

    /// Invalidates all cached directories for this `PMTiles` file.
    pub fn invalidate_all(&self) {
        self.cache.0.invalidate_all();
        log::info!("Invalidated PMTiles directory cache for id={}", self.id);
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
    async fn get_dir_entry(
        &self,
        offset: usize,
        tile_id: pmtiles::TileId,
    ) -> pmtiles::DirCacheResult {
        if let Some(dir) = self.get(offset).await {
            dir.find_tile_id(tile_id).into()
        } else {
            pmtiles::DirCacheResult::NotCached
        }
    }

    async fn insert_dir(&self, offset: usize, directory: pmtiles::Directory) {
        self.cache.0.insert(PmtCacheKey::new(self.id, offset), directory).await;
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
