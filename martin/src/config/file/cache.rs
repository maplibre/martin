use std::time::Duration;

/// Configuration for all cache types.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum size for tile cache in MB (0 to disable).
    #[cfg(feature = "_tiles")]
    pub tile_cache_size_mb: u64,
    /// Maximum lifetime for cached tiles (TTL - time to live from creation).
    /// If not set, tiles don't expire based on age.
    #[cfg(feature = "_tiles")]
    pub tile_cache_expiry: Option<Duration>,
    /// Maximum idle time for cached tiles (TTI - time to idle since last access).
    /// If not set, tiles don't expire based on idle time.
    #[cfg(feature = "_tiles")]
    pub tile_cache_idle_timeout: Option<Duration>,
    /// Maximum size for `PMTiles` directory cache in MB (0 to disable).
    #[cfg(feature = "pmtiles")]
    pub pmtiles_cache_size_mb: u64,
    /// Maximum lifetime for cached PMTiles directories (TTL - time to live from creation).
    #[cfg(feature = "pmtiles")]
    pub pmtiles_cache_expiry: Option<Duration>,
    /// Maximum idle time for cached PMTiles directories (TTI - time to idle since last access).
    #[cfg(feature = "pmtiles")]
    pub pmtiles_cache_idle_timeout: Option<Duration>,
    /// Maximum size for sprite cache in MB (0 to disable).
    #[cfg(feature = "sprites")]
    pub sprite_cache_size_mb: u64,
    /// Maximum lifetime for cached sprites (TTL - time to live from creation).
    #[cfg(feature = "sprites")]
    pub sprite_cache_expiry: Option<Duration>,
    /// Maximum idle time for cached sprites (TTI - time to idle since last access).
    #[cfg(feature = "sprites")]
    pub sprite_cache_idle_timeout: Option<Duration>,
    /// Maximum size for font cache in MB (0 to disable).
    #[cfg(feature = "fonts")]
    pub font_cache_size_mb: u64,
    /// Maximum lifetime for cached fonts (TTL - time to live from creation).
    #[cfg(feature = "fonts")]
    pub font_cache_expiry: Option<Duration>,
    /// Maximum idle time for cached fonts (TTI - time to idle since last access).
    #[cfg(feature = "fonts")]
    pub font_cache_idle_timeout: Option<Duration>,
}

impl CacheConfig {
    /// Creates tile cache if configured.
    #[cfg(feature = "_tiles")]
    #[must_use]
    pub fn create_tile_cache(&self) -> Option<martin_core::tiles::TileCache> {
        if self.tile_cache_size_mb > 0 {
            let size = self.tile_cache_size_mb * 1000 * 1000;

            let mut info_parts = vec![format!("maximum size {} MB", self.tile_cache_size_mb)];
            if let Some(ttl) = self.tile_cache_expiry {
                info_parts.push(format!("TTL {:?}", ttl));
            }
            if let Some(tti) = self.tile_cache_idle_timeout {
                info_parts.push(format!("TTI {:?}", tti));
            }

            tracing::info!("Initializing tile cache with {}", info_parts.join(", "));

            Some(martin_core::tiles::TileCache::new(
                size,
                self.tile_cache_expiry,
                self.tile_cache_idle_timeout,
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
        // TODO: make this actually disabled, not just zero sized cached
        if self.pmtiles_cache_size_mb > 0 {
            let size = self.pmtiles_cache_size_mb * 1000 * 1000;

            let mut info_parts = vec![format!("maximum size {} MB", self.pmtiles_cache_size_mb)];
            if let Some(ttl) = self.pmtiles_cache_expiry {
                info_parts.push(format!("TTL {:?}", ttl));
            }
            if let Some(tti) = self.pmtiles_cache_idle_timeout {
                info_parts.push(format!("TTI {:?}", tti));
            }

            tracing::info!(
                "Initializing PMTiles directory cache with {}",
                info_parts.join(", ")
            );

            martin_core::tiles::pmtiles::PmtCache::new(
                size,
                self.pmtiles_cache_expiry,
                self.pmtiles_cache_idle_timeout,
            )
        } else {
            tracing::debug!("PMTiles directory caching is disabled");
            martin_core::tiles::pmtiles::PmtCache::new(0, None, None)
        }
    }

    /// Creates sprite cache if configured.
    #[cfg(feature = "sprites")]
    #[must_use]
    pub fn create_sprite_cache(&self) -> martin_core::sprites::OptSpriteCache {
        if self.sprite_cache_size_mb > 0 {
            let size = self.sprite_cache_size_mb * 1000 * 1000;

            let mut info_parts = vec![format!("maximum size {} MB", self.sprite_cache_size_mb)];
            if let Some(ttl) = self.sprite_cache_expiry {
                info_parts.push(format!("TTL {:?}", ttl));
            }
            if let Some(tti) = self.sprite_cache_idle_timeout {
                info_parts.push(format!("TTI {:?}", tti));
            }

            tracing::info!("Initializing sprite cache with {}", info_parts.join(", "));

            Some(martin_core::sprites::SpriteCache::new(
                size,
                self.sprite_cache_expiry,
                self.sprite_cache_idle_timeout,
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
        if self.font_cache_size_mb > 0 {
            let size = self.font_cache_size_mb * 1000 * 1000;

            let mut info_parts = vec![format!("maximum size {} MB", self.font_cache_size_mb)];
            if let Some(ttl) = self.font_cache_expiry {
                info_parts.push(format!("TTL {:?}", ttl));
            }
            if let Some(tti) = self.font_cache_idle_timeout {
                info_parts.push(format!("TTI {:?}", tti));
            }

            tracing::info!("Initializing font cache with {}", info_parts.join(", "));

            Some(martin_core::fonts::FontCache::new(
                size,
                self.font_cache_expiry,
                self.font_cache_idle_timeout,
            ))
        } else {
            tracing::info!("Font caching is disabled");
            None
        }
    }
}
