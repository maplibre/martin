use actix_web::web::Bytes;
use thin_vec::ThinVec;

use crate::SourceId;
use crate::cache::{CacheKey, Cacheable, ResourceCache, thin_vec_heap_size};

/// Sprite cache for storing generated sprite sheets.
pub type SpriteCache = ResourceCache<SpriteCacheKey, Bytes>;

/// Optional wrapper for [`SpriteCache`].
pub type OptSpriteCache = Option<SpriteCache>;

/// Constant representing no sprite cache configuration.
pub const NO_SPRITE_CACHE: OptSpriteCache = None;

/// Cache key for sprite data.
///
/// Build it with [`crate::sprites::SpriteSources::cache_key`], which expands aliases to their member sources
/// and sorts and deduplicates them, so equivalent requests share one entry
/// and invalidating a source also evicts entries reached through an alias.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct SpriteCacheKey {
    /// The sources of the sheet, sorted by name without repeats.
    ids: ThinVec<SourceId>,
    /// Whether the sheet is the `@2x` pixel ratio.
    high_dpi: bool,
    /// Whether the sheet is rendered as signed distance fields.
    as_sdf: bool,
    /// Whether this is the sheet's JSON index rather than its PNG.
    as_json: bool,
}

impl SpriteCacheKey {
    /// Build a key from the request fields.
    #[must_use]
    pub const fn new(ids: ThinVec<SourceId>, high_dpi: bool, as_sdf: bool, as_json: bool) -> Self {
        Self {
            ids,
            high_dpi,
            as_sdf,
            as_json,
        }
    }
}

impl CacheKey for SpriteCacheKey {
    const CACHE_NAME: &'static str = "sprite";

    fn matches_source(&self, source: SourceId) -> bool {
        self.ids.contains(&source)
    }

    fn record_outcome(&self, hit: bool) {
        #[cfg(feature = "metrics")]
        crate::metrics::CACHE_REQUESTS_TOTAL
            .with_label_values(&[Self::CACHE_NAME, crate::cache::hit_miss_label(hit)])
            .inc();
        #[allow(clippy::if_same_then_else)]
        if hit {
            hotpath::gauge!("sprite_cache_hits").inc(1.0);
        } else {
            hotpath::gauge!("sprite_cache_misses").inc(1.0);
        }
    }

    fn heap_size(&self) -> usize {
        thin_vec_heap_size(&self.ids)
    }
}

impl Cacheable for Bytes {
    fn heap_size(&self) -> usize {
        self.len()
    }
}
