use martin_tile_utils::TileCoord;
use moka::future::Cache;

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
                .build(),
        )
    }

    /// Retrieves a tile from cache if present.
    pub async fn get(&self, source_id: &str, xyz: TileCoord, query: Option<&str>) -> Option<Tile> {
        let key = TileCacheKey::new(source_id, xyz, query);
        let result = self.0.get(&key).await;

        if result.is_some() {
            log::trace!(
                "Tile cache HIT for {key:?} (entries={entries}, size={size}B)",
                entries = self.0.entry_count(),
                size = self.0.weighted_size()
            );
        } else {
            log::trace!("Tile cache MISS for {key:?}, query={query:?}");
        }

        result
    }

    /// Inserts a tile into the cache.
    pub async fn insert(&self, source_id: &str, xyz: TileCoord, query: Option<&str>, data: Tile) {
        let key = TileCacheKey::new(source_id, xyz, query);
        self.0.insert(key, data).await;
    }

    /// Gets a tile from cache or computes it using the provided function.
    pub async fn get_or_insert<F, Fut, E>(
        &self,
        source_id: &str,
        xyz: TileCoord,
        query: Option<&str>,
        compute: F,
    ) -> Result<Tile, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Tile, E>>,
    {
        if let Some(data) = self.get(source_id, xyz, query).await {
            return Ok(data);
        }

        let data = compute().await?;
        self.insert(source_id, xyz, query, data.clone()).await;
        Ok(data)
    }

    /// Invalidates all cached tiles for a specific source.
    pub fn invalidate_source(&self, source_id: &str) {
        let source_id_owned = source_id.to_string();
        self.0
            .invalidate_entries_if(move |key, _| key.source_id == source_id_owned)
            .expect("invalidate_entries_if predicate should not error");
        log::info!("Invalidated tile cache for source: {source_id}");
    }

    /// Invalidates all cached tiles.
    pub fn invalidate_all(&self) {
        self.0.invalidate_all();
        log::info!("Invalidated all tile cache entries");
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
    fn new(source_id: &str, xyz: TileCoord, query: Option<&str>) -> Self {
        Self {
            source_id: source_id.to_string(),
            xyz,
            query: query.map(ToString::to_string),
        }
    }
}
