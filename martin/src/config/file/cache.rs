#[cfg(any(feature = "_tiles", feature = "fonts", feature = "sprites"))]
use std::num::NonZeroU64;
#[cfg(any(feature = "_tiles", feature = "sprites", feature = "fonts"))]
use std::time::Duration;

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
    pub fn create_tile_cache(&self) -> Option<martin_core::tiles::TileCache> {
        if let Some(setting) = &self.tiles {
            tracing::info!(
                "Initializing tile cache with maximum size {} MB",
                setting.size_mb
            );

            let size = setting.size_mb.get() * 1000 * 1000;
            Some(martin_core::tiles::TileCache::new(
                size,
                setting.expiry,
                setting.idle_timeout,
            ))
        } else {
            tracing::info!("Tile caching is disabled");
            None
        }
    }

    /// Creates `PMTiles` directory cache if configured.
    #[cfg(feature = "pmtiles")]
    #[must_use]
    pub fn create_pmtiles_cache(&self) -> martin_core::tiles::pmtiles::PmtCache {
        if let Some(setting) = &self.pmtiles {
            tracing::info!(
                "Initializing PMTiles directory cache with maximum size {} MB",
                setting.size_mb
            );
            let size = setting.size_mb.get() * 1000 * 1000;

            martin_core::tiles::pmtiles::PmtCache::new(size, setting.expiry, setting.idle_timeout)
        } else {
            tracing::debug!("PMTiles directory caching is disabled");
            // TODO: make this actually disabled, not just zero sized cached
            martin_core::tiles::pmtiles::PmtCache::new(0, None, None)
        }
    }

    /// Creates sprite cache if configured.
    #[cfg(feature = "sprites")]
    #[must_use]
    pub fn create_sprite_cache(&self) -> martin_core::sprites::OptSpriteCache {
        if let Some(setting) = &self.sprites {
            tracing::info!(
                "Initializing sprite cache with maximum size {} MB",
                setting.size_mb
            );
            let size = setting.size_mb.get() * 1000 * 1000;

            Some(martin_core::sprites::SpriteCache::new(
                size,
                setting.expiry,
                setting.idle_timeout,
            ))
        } else {
            tracing::info!("Sprite caching is disabled");
            None
        }
    }

    /// Creates font cache if configured.
    #[cfg(feature = "fonts")]
    #[must_use]
    pub fn create_font_cache(&self) -> martin_core::fonts::OptFontCache {
        if let Some(setting) = &self.fonts {
            tracing::info!(
                "Initializing font cache with maximum size {} MB",
                setting.size_mb
            );
            let size = setting.size_mb.get() * 1000 * 1000;

            Some(martin_core::fonts::FontCache::new(
                size,
                setting.expiry,
                setting.idle_timeout,
            ))
        } else {
            tracing::info!("Font caching is disabled");
            None
        }
    }
}

/// Settings for one cache
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg(any(feature = "_tiles", feature = "fonts", feature = "sprites"))]
pub struct SubCacheSetting {
    /// Maximum size for cache in MB
    pub size_mb: NonZeroU64,
    /// Maximum lifetime for cached items (TTL - time to live from creation).
    /// If not set, items don't expire based on age.
    pub expiry: Option<Duration>,
    /// Maximum idle time for cached items (TTI - time to idle since last access).
    /// If not set, items don't expire based on idle time.
    pub idle_timeout: Option<Duration>,
}
