use std::num::NonZeroU64;
use std::time::Duration;

#[cfg(feature = "fonts")]
use martin_core::fonts::{FontCache, OptFontCache};
#[cfg(feature = "sprites")]
use martin_core::sprites::{OptSpriteCache, SpriteCache};
#[cfg(feature = "_tiles")]
use martin_core::tiles::TileCache;
#[cfg(feature = "pmtiles")]
use martin_core::tiles::pmtiles::PmtCache;

/// Per-cache-type settings bundling size, TTL, and idle timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubCacheSetting {
    /// Maximum cache size in megabytes.
    pub size_mb: NonZeroU64,
    /// Maximum lifetime of a cache entry (time-to-live from creation).
    pub expiry: Option<Duration>,
    /// Maximum idle time before a cache entry is evicted (time-to-idle since last access).
    pub idle_timeout: Option<Duration>,
}

/// Configuration for all cache types.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    #[cfg(feature = "_tiles")]
    pub tiles: Option<SubCacheSetting>,
    #[cfg(feature = "pmtiles")]
    pub pmtiles: Option<SubCacheSetting>,
    #[cfg(feature = "sprites")]
    pub sprites: Option<SubCacheSetting>,
    #[cfg(feature = "fonts")]
    pub fonts: Option<SubCacheSetting>,
}

impl CacheConfig {
    /// Creates tile cache if configured.
    #[cfg(feature = "_tiles")]
    #[must_use]
    pub fn create_tile_cache(&self) -> Option<TileCache> {
        if let Some(setting) = &self.tiles {
            tracing::info!(
                "Initializing tile cache with maximum size {} MB",
                setting.size_mb
            );
            let size = setting.size_mb.get() * 1000 * 1000;
            Some(TileCache::new(size, setting.expiry, setting.idle_timeout))
        } else {
            tracing::info!("Tile caching is disabled");
            None
        }
    }

    /// Creates `PMTiles` directory cache if configured.
    #[cfg(feature = "pmtiles")]
    #[must_use]
    pub fn create_pmtiles_cache(&self) -> PmtCache {
        if let Some(setting) = &self.pmtiles {
            tracing::info!(
                "Initializing PMTiles directory cache with maximum size {} MB",
                setting.size_mb
            );
            let size = setting.size_mb.get() * 1000 * 1000;
            PmtCache::new(size, setting.expiry, setting.idle_timeout)
        } else {
            // TODO: make this actually disabled, not just zero sized cached
            tracing::debug!("PMTiles directory caching is disabled");
            PmtCache::new(0, None, None)
        }
    }

    /// Creates sprite cache if configured.
    #[cfg(feature = "sprites")]
    #[must_use]
    pub fn create_sprite_cache(&self) -> OptSpriteCache {
        if let Some(setting) = &self.sprites {
            tracing::info!(
                "Initializing sprite cache with maximum size {} MB",
                setting.size_mb
            );
            let size = setting.size_mb.get() * 1000 * 1000;
            Some(SpriteCache::new(size, setting.expiry, setting.idle_timeout))
        } else {
            tracing::info!("Sprite caching is disabled");
            None
        }
    }

    /// Creates font cache if configured.
    #[cfg(feature = "fonts")]
    #[must_use]
    pub fn create_font_cache(&self) -> OptFontCache {
        if let Some(setting) = &self.fonts {
            tracing::info!(
                "Initializing font cache with maximum size {} MB",
                setting.size_mb
            );
            let size = setting.size_mb.get() * 1000 * 1000;
            Some(FontCache::new(size, setting.expiry, setting.idle_timeout))
        } else {
            tracing::info!("Font caching is disabled");
            None
        }
    }
}
