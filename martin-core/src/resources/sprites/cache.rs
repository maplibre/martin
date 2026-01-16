use actix_web::web::Bytes;
use moka::future::Cache;
use tracing::{info, trace};

/// Sprite cache for storing generated sprite sheets.
#[derive(Clone)]
pub struct SpriteCache {
    cache: Cache<SpriteCacheKey, Bytes>,
}

impl std::fmt::Debug for SpriteCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpriteCache")
            .field("entry_count", &self.cache.entry_count())
            .field("weighted_size", &self.cache.weighted_size())
            .finish()
    }
}

impl SpriteCache {
    /// Creates a new sprite cache with the specified maximum size in bytes.
    #[must_use]
    pub fn new(max_size_bytes: u64) -> Self {
        Self {
            cache: Cache::builder()
                .name("sprite_cache")
                .weigher(|key: &SpriteCacheKey, value: &Bytes| -> u32 {
                    size_of_val(key).try_into().unwrap_or(u32::MAX)
                        + value.len().try_into().unwrap_or(u32::MAX)
                })
                .max_capacity(max_size_bytes)
                .build(),
        }
    }

    /// Retrieves a sprite sheet from cache if present.
    async fn get(&self, key: &SpriteCacheKey) -> Option<Bytes> {
        let result = self.cache.get(key).await;

        if result.is_some() {
            trace!(
                "Sprite cache HIT for {key:?} (entries={}, size={})",
                self.cache.entry_count(),
                self.cache.weighted_size()
            );
        } else {
            trace!("Sprite cache MISS for {key:?}");
        }

        result
    }

    /// Gets a json sprite sheet from cache or computes it using the provided function.
    pub async fn get_or_insert<F, Fut, E>(
        &self,
        ids: String,
        as_sdf: bool,
        as_json: bool,
        compute: F,
    ) -> Result<Bytes, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Bytes, E>>,
    {
        let key = SpriteCacheKey::new(ids, as_sdf, as_json);
        if let Some(data) = self.get(&key).await {
            return Ok(data);
        }

        let data = compute().await?;
        self.cache.insert(key, data.clone()).await;
        Ok(data)
    }

    /// Invalidates all cached sprites that use the specified source ID.
    pub fn invalidate_source(&self, source_id: &str) {
        let source_id_owned = source_id.to_string();
        self.cache
            .invalidate_entries_if(move |key, _| key.ids.contains(&source_id_owned))
            .expect("invalidate_entries_if predicate should not error");
        info!("Invalidated sprite cache for source: {source_id}");
    }

    /// Invalidates all cached sprites.
    pub fn invalidate_all(&self) {
        self.cache.invalidate_all();
        info!("Invalidated all sprite cache entries");
    }

    /// Returns the number of cached entries.
    #[must_use]
    pub fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }

    /// Returns the total size of cached data in bytes.
    #[must_use]
    pub fn weighted_size(&self) -> u64 {
        self.cache.weighted_size()
    }
}

/// Optional wrapper for `SpriteCache`.
pub type OptSpriteCache = Option<SpriteCache>;

/// Constant representing no sprite cache configuration.
pub const NO_SPRITE_CACHE: OptSpriteCache = None;

/// Cache key for sprite data.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct SpriteCacheKey {
    ids: String,
    as_sdf: bool,
    as_json: bool,
}

impl SpriteCacheKey {
    fn new(ids: String, as_sdf: bool, as_json: bool) -> Self {
        Self {
            ids,
            as_sdf,
            as_json,
        }
    }
}
