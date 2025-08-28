use log::info;
use martin_tile_utils::TileCoord;
use moka::future::Cache;

use crate::TileData;

pub type MainCache = Cache<CacheKey, CacheValue>;
pub type OptMainCache = Option<MainCache>;
pub const NO_MAIN_CACHE: OptMainCache = None;

/// Constructs a main cache with the specified size in megabytes.
///
/// If the size is zero, caching is disabled.
/// Logs initialization and capacity.
#[must_use]
pub fn construct_cache(cache_size_mb: Option<u64>) -> OptMainCache {
    let cache_size = cache_size_mb.unwrap_or(512) * 1024 * 1024;
    if cache_size > 0 {
        info!("Initializing main cache with maximum size {cache_size}B");
        Some(
            MainCache::builder()
                .weigher(|_key, value: &CacheValue| -> u32 {
                    match value {
                        CacheValue::Tile(v) => v.len().try_into().unwrap_or(u32::MAX),
                        #[cfg(feature = "pmtiles")]
                        CacheValue::PmtDirectory(v) => {
                            v.get_approx_byte_size().try_into().unwrap_or(u32::MAX)
                        }
                    }
                })
                .max_capacity(cache_size)
                .build(),
        )
    } else {
        info!("Caching is disabled");
        None
    }
}

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum CacheKey {
    /// (`pmtiles_id`, `offset`)
    PmtDirectory(usize, usize),
    /// (`source_id`, `xyz`)
    Tile(String, TileCoord),
    /// (`source_id`, `xyz`, `url_query`)
    TileWithQuery(String, TileCoord, String),
}

#[derive(Debug, Clone)]
pub enum CacheValue {
    Tile(TileData),
    #[cfg(feature = "pmtiles")]
    PmtDirectory(pmtiles::Directory),
}

macro_rules! trace_cache {
    ($typ: literal, $cache: expr, $key: expr) => {
        trace!(
            "Cache {} for {:?} in {:?} that has {} entries taking up {} space",
            $typ,
            $key,
            $cache.name(),
            $cache.entry_count(),
            $cache.weighted_size(),
        );
    };
}

macro_rules! from_cache_value {
    ($value_type: path, $data: expr, $key: expr) => {
        #[allow(irrefutable_let_patterns)]
        if let $value_type(data) = $data {
            data
        } else {
            panic!("Unexpected value type {:?} for key {:?} cache", $data, $key)
        }
    };
}

#[cfg(feature = "pmtiles")]
macro_rules! get_cached_value {
    ($cache: expr, $value_type: path, $make_key: expr) => {
        if let Some(cache) = $cache {
            let key = $make_key;
            if let Some(data) = cache.get(&key).await {
                $crate::utils::cache::trace_cache!("HIT", cache, key);
                Some($crate::utils::cache::from_cache_value!(
                    $value_type,
                    data,
                    key
                ))
            } else {
                $crate::utils::cache::trace_cache!("MISS", cache, key);
                None
            }
        } else {
            None
        }
    };
}

macro_rules! get_or_insert_cached_value {
    ($cache: expr, $value_type: path, $make_item:expr, $make_key: expr) => {{
        if let Some(cache) = $cache {
            let key = $make_key;
            Ok(if let Some(data) = cache.get(&key).await {
                $crate::utils::cache::trace_cache!("HIT", cache, key);
                $crate::utils::cache::from_cache_value!($value_type, data, key)
            } else {
                $crate::utils::cache::trace_cache!("MISS", cache, key);
                let data = $make_item.await?;
                cache.insert(key, $value_type(data.clone())).await;
                data
            })
        } else {
            $make_item.await
        }
    }};
}

#[cfg(feature = "pmtiles")]
pub(crate) use get_cached_value;
pub(crate) use {from_cache_value, get_or_insert_cached_value, trace_cache};
