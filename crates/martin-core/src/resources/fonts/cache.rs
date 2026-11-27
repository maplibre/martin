use bytes::Bytes;
use martin_tile_utils::Encoding;
use thin_vec::ThinVec;

use crate::SourceId;
use crate::cache::{CacheKey, ResourceCache, thin_vec_heap_size};

/// Font cache for storing generated font ranges (PBF glyph data).
pub type FontCache = ResourceCache<FontCacheKey, Bytes>;

/// Optional wrapper for [`FontCache`].
pub type OptFontCache = Option<FontCache>;

/// Constant representing no font cache configuration.
pub const NO_FONT_CACHE: OptFontCache = None;

/// Cache key for a font glyph range.
///
/// Build it with [`crate::fonts::FontSources::cache_key`], which expands aliases to their member fonts,
/// so invalidating a font also evicts entries reached through an alias.
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct FontCacheKey {
    /// The fonts of the stack, in fallback order without repeats.
    ids: ThinVec<SourceId>,
    start: u32,
    end: u32,
    /// The content encoding the range is cached in, `None` for the glyphs as generated.
    encoding: Option<Encoding>,
}

impl FontCacheKey {
    /// Build a key for the given font stack and glyph range.
    #[must_use]
    pub const fn new(ids: ThinVec<SourceId>, start: u32, end: u32) -> Self {
        Self {
            ids,
            start,
            end,
            encoding: None,
        }
    }

    /// The key of the same glyph range compressed with `encoding`.
    #[must_use]
    pub fn with_encoding(self, encoding: Encoding) -> Self {
        Self {
            encoding: Some(encoding),
            ..self
        }
    }
}

impl CacheKey for FontCacheKey {
    const CACHE_NAME: &'static str = "font";

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
            hotpath::gauge!("font_cache_hits").inc(1.0);
        } else {
            hotpath::gauge!("font_cache_misses").inc(1.0);
        }
    }

    fn heap_size(&self) -> usize {
        thin_vec_heap_size(&self.ids)
    }
}
