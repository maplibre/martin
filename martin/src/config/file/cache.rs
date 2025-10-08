/// Configuration for all cache types.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum size for tile cache in MB (0 to disable).
    #[cfg(any(
        feature = "postgres",
        feature = "pmtiles",
        feature = "mbtiles",
        feature = "unstable-cog"
    ))]
    pub tile_cache_size_mb: u64,
    /// Maximum size for PMTiles directory cache in MB (0 to disable).
    #[cfg(feature = "pmtiles")]
    pub pmtiles_cache_size_mb: u64,
    /// Maximum size for sprite cache in MB (0 to disable).
    #[cfg(feature = "sprites")]
    pub sprite_cache_size_mb: u64,
    /// Maximum size for font cache in MB (0 to disable).
    #[cfg(feature = "fonts")]
    pub font_cache_size_mb: u64,
}

impl CacheConfig {
    /// Creates tile cache if configured.
    #[cfg(any(
        feature = "postgres",
        feature = "pmtiles",
        feature = "mbtiles",
        feature = "unstable-cog"
    ))]
    #[must_use]
    pub fn create_tile_cache(&self) -> Option<martin_core::tiles::TileCache> {
        if self.tile_cache_size_mb > 0 {
            let size = self.tile_cache_size_mb * 1024 * 1024;
            log::info!("Initializing tile cache with maximum size {size}B");
            Some(martin_core::tiles::TileCache::new(size))
        } else {
            log::info!("Tile caching is disabled");
            None
        }
    }

    /// Creates PMTiles directory cache if configured.
    #[cfg(feature = "pmtiles")]
    #[must_use]
    pub fn create_pmtiles_cache(&self) -> martin_core::tiles::pmtiles::PmtCache {
        // TODO: make this actually disabled, not just zero sized cached
        if self.pmtiles_cache_size_mb > 0 {
            let size = self.pmtiles_cache_size_mb * 1024 * 1024;
            log::info!("Initializing PMTiles directory cache with maximum size {size}B");
            martin_core::tiles::pmtiles::PmtCache::new(size)
        } else {
            log::debug!("PMTiles directory caching is disabled");
            martin_core::tiles::pmtiles::PmtCache::new(0)
        }
    }

    /// Creates sprite cache if configured.
    #[cfg(feature = "sprites")]
    #[must_use]
    pub fn create_sprite_cache(&self) -> martin_core::sprites::OptSpriteCache {
        if self.sprite_cache_size_mb > 0 {
            let size = self.sprite_cache_size_mb * 1024 * 1024;
            log::info!("Initializing sprite cache with maximum size {size}B");
            Some(martin_core::sprites::SpriteCache::new(size))
        } else {
            log::info!("Sprite caching is disabled");
            None
        }
    }

    /// Creates font cache if configured.
    #[cfg(feature = "fonts")]
    #[must_use]
    pub fn create_font_cache(&self) -> martin_core::fonts::OptFontCache {
        if self.font_cache_size_mb > 0 {
            let size = self.font_cache_size_mb * 1024 * 1024;
            log::info!("Initializing font cache with maximum size {size}B");
            Some(martin_core::fonts::FontCache::new(size))
        } else {
            log::info!("Font caching is disabled");
            None
        }
    }
}
