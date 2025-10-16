use martin_tile_utils::TileCoord;
use moka::future::Cache;

use crate::tiles::Tile;

/// Main cache instance for storing tiles and `PMTiles` directories.
pub type TileCache = Cache<CacheKey, CacheValue>;

/// Optional wrapper for the [`TileCache`].
pub type OptTileCache = Option<TileCache>;

/// Constant representing no cache configuration.
pub const NO_TILE_CACHE: OptTileCache = None;

/// Keys used to identify cached items.
#[derive(Debug, Hash, PartialEq, Eq)]
pub enum CacheKey {
    /// `PMTiles` directory cache key with `PMTiles ID` and `offset`.
    #[cfg(feature = "pmtiles")]
    PmtDirectory(usize, usize),
    /// Tile cache key with `source ID` and `coordinates`.
    #[cfg(feature = "_tiles")]
    Tile(String, TileCoord),
    /// Tile cache key with `source ID`, [`TileCoord`], and `URL query parameters`.
    #[cfg(feature = "_tiles")]
    TileWithQuery(String, TileCoord, String),
}

/// Values stored in the cache.
#[derive(Debug, Clone)]
pub enum CacheValue {
    /// Cached tile data.
    #[cfg(feature = "_tiles")]
    Tile(Tile),
    /// Cached `PMTiles` directory.
    #[cfg(feature = "pmtiles")]
    PmtDirectory(pmtiles::Directory),
}

/// Logs cache operation details for debugging and monitoring.
#[inline]
pub fn trace_cache(typ: &'static str, cache: &TileCache, key: &CacheKey) {
    log::trace!(
        "Cache {typ} for {key:?} in {name:?} that has {entry_count} entries taking up {weighted_size} space",
        name = cache.name(),
        entry_count = cache.entry_count(),
        weighted_size = cache.weighted_size(),
    );
}

/// Extracts typed data from cache values with panic on type mismatch.
#[macro_export]
macro_rules! from_cache_value {
    ($value_type: path, $data: expr, $key: expr) => {
        if let $value_type(data) = $data {
            data
        } else {
            panic!("Unexpected value type {:?} for key {:?} cache", $data, $key)
        }
    };
}

/// Retrieves a value from cache if present, returning None on cache miss.
#[cfg(feature = "pmtiles")]
#[macro_export]
macro_rules! get_cached_value {
    ($cache: expr, $value_type: path, $make_key: expr) => {
        if let Some(cache) = $cache {
            let key = $make_key;
            if let Some(data) = cache.get(&key).await {
                $crate::tiles::cache::trace_cache("HIT", cache, &key);
                Some($crate::from_cache_value!($value_type, data, key))
            } else {
                $crate::tiles::cache::trace_cache("MISS", cache, &key);
                None
            }
        } else {
            None
        }
    };
}

/// Gets a value from cache or computes and inserts it on cache miss.
#[macro_export]
macro_rules! get_or_insert_cached_value {
    ($cache: expr, $value_type: path, $make_item:expr, $make_key: expr) => {{
        if let Some(cache) = $cache {
            let key = $make_key;
            Ok(if let Some(data) = cache.get(&key).await {
                $crate::tiles::trace_cache("HIT", cache, &key);
                $crate::from_cache_value!($value_type, data, key)
            } else {
                $crate::tiles::trace_cache("MISS", cache, &key);
                let data = $make_item.await?;
                cache.insert(key, $value_type(data.clone())).await;
                data
            })
        } else {
            $make_item.await
        }
    }};
}
