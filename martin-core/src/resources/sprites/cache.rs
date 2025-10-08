use std::sync::Arc;

use moka::future::Cache;

/// Sprite cache for storing generated sprite sheets.

#[derive(Clone)]
pub struct SpriteCache {
    cache: Cache<SpriteCacheKey, Arc<spreet::Spritesheet>>,
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
                .weigher(
                    |_key: &SpriteCacheKey, value: &Arc<spreet::Spritesheet>| -> u32 {
                        // Approximate size: JSON index + PNG data
                        let json_size = serde_json::to_string(value.get_index())
                            .map(|s| s.len())
                            .unwrap_or(0);
                        let png_size = value.encode_png().map(|p| p.len()).unwrap_or(0);
                        (json_size + png_size).try_into().unwrap_or(u32::MAX)
                    },
                )
                .max_capacity(max_size_bytes)
                .build(),
        }
    }

    /// Retrieves a sprite sheet from cache if present.
    pub async fn get(
        &self,
        ids: &str,
        pixel_ratio: u8,
        as_sdf: bool,
    ) -> Option<Arc<spreet::Spritesheet>> {
        let key = SpriteCacheKey::new(ids, pixel_ratio, as_sdf);
        let result = self.cache.get(&key).await;

        if result.is_some() {
            log::trace!(
                "Sprite cache HIT for ids={ids}, ratio={pixel_ratio}, sdf={as_sdf} (entries={}, size={})",
                self.cache.entry_count(),
                self.cache.weighted_size()
            );
        } else {
            log::trace!("Sprite cache MISS for ids={ids}, ratio={pixel_ratio}, sdf={as_sdf}");
        }

        result
    }

    /// Inserts a sprite sheet into the cache.
    pub async fn insert(
        &self,
        ids: &str,
        pixel_ratio: u8,
        as_sdf: bool,
        spritesheet: Arc<spreet::Spritesheet>,
    ) {
        let key = SpriteCacheKey::new(ids, pixel_ratio, as_sdf);
        self.cache.insert(key, spritesheet).await;
    }

    /// Gets a sprite sheet from cache or computes it using the provided function.
    pub async fn get_or_insert<F, Fut, E>(
        &self,
        ids: &str,
        pixel_ratio: u8,
        as_sdf: bool,
        compute: F,
    ) -> Result<Arc<spreet::Spritesheet>, E>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<spreet::Spritesheet, E>>,
    {
        if let Some(data) = self.get(ids, pixel_ratio, as_sdf).await {
            return Ok(data);
        }

        let data = Arc::new(compute().await?);
        self.insert(ids, pixel_ratio, as_sdf, data.clone()).await;
        Ok(data)
    }

    /// Invalidates all cached sprites that use the specified source ID.
    pub async fn invalidate_source(&self, source_id: &str) {
        let source_id_owned = source_id.to_string();
        self.cache
            .invalidate_entries_if(move |key, _| key.ids.contains(&source_id_owned))
            .expect("invalidate_entries_if predicate should not error");
        log::info!("Invalidated sprite cache for source: {source_id}");
    }

    /// Invalidates all cached sprites.
    pub async fn invalidate_all(&self) {
        self.cache.invalidate_all();
        log::info!("Invalidated all sprite cache entries");
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
    pixel_ratio: u8,
    as_sdf: bool,
}

impl SpriteCacheKey {
    fn new(ids: &str, pixel_ratio: u8, as_sdf: bool) -> Self {
        Self {
            ids: ids.to_string(),
            pixel_ratio,
            as_sdf,
        }
    }
}
