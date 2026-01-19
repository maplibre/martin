use std::time::Duration;

use martin_tile_utils::TileCoord;
use moka::future::Cache;
use tracing::{info, trace};

use crate::tiles::Tile;

/// Tile cache for storing rendered tile data.
#[derive(Clone, Debug)]
pub struct TileCache(Cache<TileCacheKey, Tile>);

impl TileCache {
    /// Creates a new tile cache with the specified maximum size in bytes.
    ///
    /// # Arguments
    ///
    /// * `max_size_bytes` - Maximum cache size in bytes (based on tile data size)
    /// * `expiry` - Optional maximum lifetime (TTL - time to live from creation)
    /// * `idle_timeout` - Optional idle timeout (TTI - time to idle since last access)
    #[must_use]
    pub fn new(
        max_size_bytes: u64,
        expiry: Option<Duration>,
        idle_timeout: Option<Duration>,
    ) -> Self {
        let mut builder = Cache::builder()
            .name("tile_cache")
            .weigher(|_key: &TileCacheKey, value: &Tile| -> u32 {
                value.data.len().try_into().unwrap_or(u32::MAX)
            })
            .max_capacity(max_size_bytes);

        if let Some(ttl) = expiry {
            builder = builder.time_to_live(ttl);
            trace!("Tile cache configured with TTL of {:?}", ttl);
        }

        if let Some(tti) = idle_timeout {
            builder = builder.time_to_idle(tti);
            trace!("Tile cache configured with TTI of {:?}", tti);
        }

        Self(builder.build())
    }

    /// Retrieves a tile from cache if present.
    async fn get(&self, key: &TileCacheKey) -> Option<Tile> {
        let result = self.0.get(key).await;

        if result.is_some() {
            trace!(
                "Tile cache HIT for {key:?} (entries={entries}, size={size}B)",
                entries = self.0.entry_count(),
                size = self.0.weighted_size()
            );
        } else {
            trace!("Tile cache MISS for {key:?}");
        }

        result
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
        if let Some(data) = self.get(&key).await {
            return Ok(data);
        }

        let data = compute().await?;
        self.0.insert(key, data.clone()).await;
        Ok(data)
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use martin_tile_utils::{Encoding, Format, TileInfo};

    use super::*;

    #[tokio::test]
    async fn test_cache_with_ttl() {
        // Create cache with 1 second TTL
        let cache = TileCache::new(1_000_000, Some(Duration::from_secs(1)), None);

        let tile = Tile::new_hash_etag(
            vec![1, 2, 3],
            TileInfo::new(Format::Png, Encoding::Uncompressed),
        );

        let key = TileCacheKey::new(
            "test_source".to_string(),
            TileCoord { z: 0, x: 0, y: 0 },
            None,
        );

        // Insert and retrieve immediately - should work
        let result = cache
            .get_or_insert(
                "test_source".to_string(),
                TileCoord { z: 0, x: 0, y: 0 },
                None,
                || async { Ok::<_, ()>(tile.clone()) },
            )
            .await;

        assert!(result.is_ok());
        assert!(cache.get(&key).await.is_some());

        // Wait for expiry (longer to ensure expiration)
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Force eviction by running Moka's maintenance
        cache.0.run_pending_tasks().await;

        // Entry should be expired - verify it's not retrievable
        assert!(cache.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_with_tti() {
        // Create cache with 500ms idle timeout
        let cache = TileCache::new(1_000_000, None, Some(Duration::from_millis(500)));

        let tile = Tile::new_hash_etag(
            vec![1, 2, 3],
            TileInfo::new(Format::Png, Encoding::Uncompressed),
        );

        // Insert tile
        cache
            .get_or_insert(
                "test_source".to_string(),
                TileCoord { z: 0, x: 0, y: 0 },
                None,
                || async { Ok::<_, ()>(tile.clone()) },
            )
            .await
            .unwrap();

        // Access it before timeout
        tokio::time::sleep(Duration::from_millis(300)).await;
        let key = TileCacheKey::new(
            "test_source".to_string(),
            TileCoord { z: 0, x: 0, y: 0 },
            None,
        );
        cache.get(&key).await;

        // Should still be present (idle timer reset)
        assert_eq!(cache.entry_count(), 1);

        // Wait for idle timeout without accessing
        tokio::time::sleep(Duration::from_millis(600)).await;
        cache.0.run_pending_tasks().await;

        // Entry should be expired
        assert_eq!(cache.entry_count(), 0);
    }

    #[tokio::test]
    async fn test_cache_with_both_ttl_and_tti() {
        // Create cache with 2s TTL and 500ms TTI
        // Entry should expire at earliest time (500ms idle)
        let cache = TileCache::new(
            1_000_000,
            Some(Duration::from_secs(2)),
            Some(Duration::from_millis(500)),
        );

        let tile = Tile::new_hash_etag(
            vec![1, 2, 3],
            TileInfo::new(Format::Png, Encoding::Uncompressed),
        );

        cache
            .get_or_insert(
                "test_source".to_string(),
                TileCoord { z: 0, x: 0, y: 0 },
                None,
                || async { Ok::<_, ()>(tile.clone()) },
            )
            .await
            .unwrap();

        // Wait past idle timeout but before TTL
        tokio::time::sleep(Duration::from_millis(600)).await;
        cache.0.run_pending_tasks().await;

        // Should expire due to idle timeout (earliest)
        assert_eq!(cache.entry_count(), 0);
    }

    #[test]
    fn test_cache_no_expiry() {
        // Create cache with no time-based expiry
        let cache = TileCache::new(1_000_000, None, None);

        // Should only evict based on size, not time
        assert_eq!(cache.entry_count(), 0);
    }

    #[tokio::test]
    async fn test_cache_expiry_integration_hot_tiles() {
        // Test that hot tiles (frequently accessed) still expire after TTL
        let cache = TileCache::new(
            1_000_000,
            Some(Duration::from_millis(500)),  // TTL: 500ms
            Some(Duration::from_millis(2000)), // TTI: 2s (longer than TTL)
        );

        let tile = Tile::new_hash_etag(
            vec![1, 2, 3],
            TileInfo::new(Format::Png, Encoding::Uncompressed),
        );

        let key = TileCacheKey::new(
            "hot_source".to_string(),
            TileCoord { z: 0, x: 0, y: 0 },
            None,
        );

        // Insert tile
        cache
            .get_or_insert(
                "hot_source".to_string(),
                TileCoord { z: 0, x: 0, y: 0 },
                None,
                || async { Ok::<_, ()>(tile.clone()) },
            )
            .await
            .unwrap();

        // Keep accessing it (simulate hot tile)
        for _ in 0..5 {
            tokio::time::sleep(Duration::from_millis(100)).await;
            cache.get(&key).await; // Keep it "hot"
        }

        // Even though we kept accessing it, TTL should cause expiry
        tokio::time::sleep(Duration::from_millis(100)).await;
        cache.0.run_pending_tasks().await;

        // Should be expired due to TTL (500ms passed)
        assert!(cache.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_cache_expiry_integration_size_and_time() {
        // Test that both size and time-based eviction work together
        let cache = TileCache::new(
            100,                              // Very small cache (100 bytes)
            Some(Duration::from_secs(10)),    // Long TTL
            Some(Duration::from_millis(500)), // Short TTI
        );

        let large_tile = Tile::new_hash_etag(
            vec![0; 50], // 50 bytes
            TileInfo::new(Format::Png, Encoding::Uncompressed),
        );

        // Insert multiple tiles
        for i in 0..3 {
            cache
                .get_or_insert(
                    format!("source_{i}"),
                    TileCoord { z: 0, x: i, y: 0 },
                    None,
                    || async { Ok::<_, ()>(large_tile.clone()) },
                )
                .await
                .unwrap();
            cache.0.run_pending_tasks().await;
        }

        // Cache should have entries
        let count = cache.entry_count();
        assert!(count > 0, "Expected entries in cache, got {count}");

        // Wait for TTI expiry
        tokio::time::sleep(Duration::from_millis(600)).await;
        cache.0.run_pending_tasks().await;

        // Should be expired due to TTI
        assert_eq!(cache.entry_count(), 0);
    }
}
