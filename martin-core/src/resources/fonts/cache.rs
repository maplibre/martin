use crate::cache::{CacheKey, ResourceCache};

/// Font cache for storing generated font ranges (PBF glyph data).
pub type FontCache = ResourceCache<FontCacheKey, Vec<u8>>;

/// Optional wrapper for [`FontCache`].
pub type OptFontCache = Option<FontCache>;

/// Constant representing no font cache configuration.
pub const NO_FONT_CACHE: OptFontCache = None;

/// Cache key for a font glyph range.
///
/// `ids` is the comma-joined font stack from the request path. Invalidation
/// by font ID matches by token (not substring): invalidating `"Open Sans"`
/// does not invalidate entries keyed against `"Open Sans Bold"`.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct FontCacheKey {
    ids: String,
    start: u32,
    end: u32,
}

impl FontCacheKey {
    /// Build a key for the given font stack and glyph range.
    #[must_use]
    pub fn new(ids: String, start: u32, end: u32) -> Self {
        Self { ids, start, end }
    }
}

impl CacheKey for FontCacheKey {
    const CACHE_NAME: &'static str = "font";

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
            hotpath::gauge!("font_cache_hits").inc(1.0);
        } else {
            hotpath::gauge!("font_cache_misses").inc(1.0);
        }
    }
}
