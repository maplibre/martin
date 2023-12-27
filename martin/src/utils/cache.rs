use moka::future::Cache;
use pmtiles::Directory;

use crate::{TileCoord, TileData};

pub type MainCache = Cache<CacheKey, CacheValue>;
pub type OptMainCache = Option<MainCache>;
pub const NO_MAIN_CACHE: OptMainCache = None;

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum CacheKey {
    /// (pmtiles_id, offset)
    PmtDirectory(usize, usize),
    /// (source_id, xyz)
    Tile(String, TileCoord),
    /// (source_id, xyz, url_query)
    TileWithQuery(String, TileCoord, String),
}

#[derive(Debug, Clone)]
pub enum CacheValue {
    Tile(TileData),
    PmtDirectory(Directory),
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
    ($cache: expr, $value_type: path, $make_item:expr, $make_key: expr) => {
        async {
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
        }
    };
}

#[cfg(feature = "pmtiles")]
pub(crate) use get_cached_value;

pub(crate) use {from_cache_value, get_or_insert_cached_value, trace_cache};
