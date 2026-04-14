#[cfg(feature = "fonts")]
use martin_core::fonts::{FontCache, OptFontCache};
#[cfg(feature = "sprites")]
use martin_core::sprites::{OptSpriteCache, SpriteCache};
#[cfg(feature = "_tiles")]
use martin_core::tiles::TileCache;
#[cfg(feature = "pmtiles")]
use martin_core::tiles::pmtiles::PmtCache;

/// Configuration for all cache types.
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum size for tile cache in MB (0 to disable).
    #[cfg(feature = "_tiles")]
    pub tile_cache_size_mb: u64,
    /// Maximum size for `PMTiles` directory cache in MB (0 to disable).
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
    #[cfg(feature = "_tiles")]
    #[must_use]
    pub fn create_tile_cache(&self) -> Option<TileCache> {
        if self.tile_cache_size_mb > 0 {
            tracing::info!(
                "Initializing tile cache with maximum size {} MB",
                self.tile_cache_size_mb
            );
            let size = self.tile_cache_size_mb * 1000 * 1000;
            Some(TileCache::new(size))
        } else {
            tracing::info!("Tile caching is disabled");
            None
        }
    }

    /// Creates `PMTiles` directory cache if configured.
    #[cfg(feature = "pmtiles")]
    #[must_use]
    pub fn create_pmtiles_cache(&self) -> PmtCache {
        // TODO: make this actually disabled, not just zero sized cached
        if self.pmtiles_cache_size_mb > 0 {
            tracing::info!(
                "Initializing PMTiles directory cache with maximum size {} MB",
                self.pmtiles_cache_size_mb
            );
            let size = self.pmtiles_cache_size_mb * 1000 * 1000;
            PmtCache::new(size)
        } else {
            tracing::debug!("PMTiles directory caching is disabled");
            PmtCache::new(0)
        }
    }

    /// Creates sprite cache if configured.
    #[cfg(feature = "sprites")]
    #[must_use]
    pub fn create_sprite_cache(&self) -> OptSpriteCache {
        if self.sprite_cache_size_mb > 0 {
            tracing::info!(
                "Initializing sprite cache with maximum size {} MB",
                self.sprite_cache_size_mb
            );
            let size = self.sprite_cache_size_mb * 1000 * 1000;
            Some(SpriteCache::new(size))
        } else {
            tracing::info!("Sprite caching is disabled");
            None
        }
    }

    /// Creates font cache if configured.
    #[cfg(feature = "fonts")]
    #[must_use]
    pub fn create_font_cache(&self) -> OptFontCache {
        if self.font_cache_size_mb > 0 {
            tracing::info!(
                "Initializing font cache with maximum size {} MB",
                self.font_cache_size_mb
            );
            let size = self.font_cache_size_mb * 1000 * 1000;
            Some(FontCache::new(size))
        } else {
            tracing::info!("Font caching is disabled");
            None
        }
    }
}
