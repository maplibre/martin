use martin_tile_utils::{TileCoord, TileData};
use moka::future::Cache;

pub type MainCache = Cache<CacheKey, CacheValue>;
pub type OptMainCache = Option<MainCache>;
pub const NO_MAIN_CACHE: OptMainCache = None;

#[derive(Debug, Hash, PartialEq, Eq)]
pub enum CacheKey {
    #[cfg(feature = "pmtiles")]
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

pub fn trace_cache(typ: &'static str, cache: &MainCache, key: &CacheKey) {
    log::trace!(
        "Cache {typ} for {key:?} in {name:?} that has {entry_count} entries taking up {weighted_size} space",
        name = cache.name(),
        entry_count = cache.entry_count(),
        weighted_size = cache.weighted_size(),
    );
}

#[macro_export]
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
#[macro_export]
macro_rules! get_cached_value {
    ($cache: expr, $value_type: path, $make_key: expr) => {
        if let Some(cache) = $cache {
            let key = $make_key;
            if let Some(data) = cache.get(&key).await {
                $crate::cache::trace_cache("HIT", cache, &key);
                Some($crate::from_cache_value!($value_type, data, key))
            } else {
                $crate::cache::trace_cache("MISS", cache, &key);
                None
            }
        } else {
            None
        }
    };
}

#[macro_export]
macro_rules! get_or_insert_cached_value {
    ($cache: expr, $value_type: path, $make_item:expr, $make_key: expr) => {{
        if let Some(cache) = $cache {
            let key = $make_key;
            Ok(if let Some(data) = cache.get(&key).await {
                $crate::cache::trace_cache("HIT", cache, &key);
                $crate::from_cache_value!($value_type, data, key)
            } else {
                $crate::cache::trace_cache("MISS", cache, &key);
                let data = $make_item.await?;
                cache.insert(key, $value_type(data.clone())).await;
                data
            })
        } else {
            $make_item.await
        }
    }};
}
