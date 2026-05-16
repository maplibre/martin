use actix_web::web::Bytes;

use crate::cache::{CacheKey, Cacheable, ResourceCache};

/// Sprite cache for storing generated sprite sheets.
pub type SpriteCache = ResourceCache<SpriteCacheKey, Bytes>;

/// Optional wrapper for [`SpriteCache`].
pub type OptSpriteCache = Option<SpriteCache>;

/// Constant representing no sprite cache configuration.
pub const NO_SPRITE_CACHE: OptSpriteCache = None;

/// Cache key for sprite data.
///
/// `ids` is the comma-joined list of sprite source IDs from the request path.
/// Invalidation by source ID matches by token (not substring): invalidating
/// `"foo"` does not invalidate entries keyed against `"foobar"`.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct SpriteCacheKey {
    ids: String,
    as_sdf: bool,
    as_json: bool,
}

impl SpriteCacheKey {
    /// Build a key from the request fields.
    #[must_use]
    pub fn new(ids: String, as_sdf: bool, as_json: bool) -> Self {
        Self {
            ids,
            as_sdf,
            as_json,
        }
    }
}

impl CacheKey for SpriteCacheKey {
    const CACHE_NAME: &'static str = "sprite";

    fn matches_source(&self, source_id: &str) -> bool {
        self.ids.split(',').any(|s| s == source_id)
    }

    fn record_outcome(&self, hit: bool) {
        #[cfg(feature = "metrics")]
        crate::metrics::CACHE_REQUESTS_TOTAL
            .with_label_values(&[Self::CACHE_NAME, crate::cache::hit_miss_label(hit)])
            .inc();
        #[allow(
            clippy::if_same_then_else,
            reason = "hotpath::gauge! requires a literal name argument"
        )]
        if hit {
            hotpath::gauge!("sprite_cache_hits").inc(1.0);
        } else {
            hotpath::gauge!("sprite_cache_misses").inc(1.0);
        }
    }
}

impl Cacheable for Bytes {
    fn weight(&self) -> u32 {
        self.len().try_into().unwrap_or(u32::MAX)
    }
}
