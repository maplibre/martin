use std::time::Duration;

use moka::future::Cache;
use tracing::{info, trace};

/// Optional wrapper for `FontCache`.
pub type OptFontCache = Option<FontCache>;

/// Constant representing no font cache configuration.
pub const NO_FONT_CACHE: OptFontCache = None;

/// Font cache for storing generated font ranges.
#[derive(Clone, Debug)]
pub struct FontCache {
    cache: Cache<FontCacheKey, Vec<u8>>,
}

impl FontCache {
    /// Creates a new font cache with the specified maximum size in bytes.
    ///
    /// # Arguments
    ///
    /// * `max_size_bytes` - Maximum cache size in bytes (based on font data size)
    /// * `expiry` - Optional maximum lifetime (TTL - time to live from creation)
    /// * `idle_timeout` - Optional idle timeout (TTI - time to idle since last access)
    #[must_use]
    pub fn new(
        max_size_bytes: u64,
        expiry: Option<Duration>,
        idle_timeout: Option<Duration>,
    ) -> Self {
        let mut builder = Cache::builder()
            .name("font_cache")
            .weigher(|_key: &FontCacheKey, value: &Vec<u8>| -> u32 {
                value.len().try_into().unwrap_or(u32::MAX)
            })
            .max_capacity(max_size_bytes);

        if let Some(ttl) = expiry {
            builder = builder.time_to_live(ttl);
            trace!("Font cache configured with TTL of {:?}", ttl);
        }

        if let Some(tti) = idle_timeout {
            builder = builder.time_to_idle(tti);
            trace!("Font cache configured with TTI of {:?}", tti);
        }

        Self {
            cache: builder.build(),
        }
    }

    /// Retrieves a font range from cache if present.
    async fn get(&self, key: &FontCacheKey) -> Option<Vec<u8>> {
        let result = self.cache.get(key).await;

        if result.is_some() {
            trace!(
                "Font cache HIT for {key:?} (entries={}, size={})",
                self.cache.entry_count(),
                self.cache.weighted_size()
            );
        } else {
            trace!("Font cache MISS for {key:?}");
        }

        result
    }

    /// Gets a font range from cache or computes it using the provided function.
    pub async fn get_or_insert<F, E>(
        &self,
        ids: String,
        start: u32,
        end: u32,
        compute: F,
    ) -> Result<Vec<u8>, E>
    where
        F: FnOnce() -> Result<Vec<u8>, E>,
    {
        let key = FontCacheKey::new(ids, start, end);
        if let Some(data) = self.get(&key).await {
            return Ok(data);
        }

        let data = compute()?;
        self.cache.insert(key, data.clone()).await;
        Ok(data)
    }

    /// Invalidates all cached font ranges that use the specified font ID.
    pub fn invalidate_font(&self, font_id: &str) {
        let font_id_owned = font_id.to_string();
        self.cache
            .invalidate_entries_if(move |key, _| key.ids.contains(&font_id_owned))
            .expect("invalidate_entries_if predicate should not error");
        info!("Invalidated font cache for font: {font_id}");
    }

    /// Invalidates all cached font ranges.
    pub fn invalidate_all(&self) {
        self.cache.invalidate_all();
        info!("Invalidated all font cache entries");
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

/// Cache key for font data.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct FontCacheKey {
    ids: String,
    start: u32,
    end: u32,
}

impl FontCacheKey {
    fn new(ids: String, start: u32, end: u32) -> Self {
        Self { ids, start, end }
    }
}
