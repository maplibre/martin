use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

/// Configuration for all cache types.
#[derive(Debug, Clone)]
pub struct ResolvedCacheConfig {
    #[cfg(feature = "_tiles")]
    pub tiles: Option<ResolvedSubCacheSetting>,
    #[cfg(feature = "pmtiles")]
    pub pmtile_directorys: Option<ResolvedSubCacheSetting>,
    #[cfg(feature = "sprites")]
    pub sprites: Option<ResolvedSubCacheSetting>,
    #[cfg(feature = "fonts")]
    pub fonts: Option<ResolvedSubCacheSetting>,
}

impl ResolvedCacheConfig {
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
            Some(martin_core::tiles::TileCache::new(size))
        } else {
            tracing::info!("Tile caching is disabled");
            None
        }
    }

    /// Creates `PMTiles` directory cache if configured.
    #[cfg(feature = "pmtiles")]
    #[must_use]
    pub fn create_pmtile_directorys_cache(&self) -> martin_core::tiles::pmtiles::PmtCache {
        // TODO: make this actually disabled, not just zero sized cached
        if let Some(setting) = &self.pmtile_directorys {
            tracing::info!(
                "Initializing PMTiles directory cache with maximum size {} MB",
                setting.size_mb
            );
            let size = setting.size_mb.get() * 1000 * 1000;
            martin_core::tiles::pmtiles::PmtCache::new(size)
        } else {
            tracing::debug!("PMTiles directory caching is disabled");
            martin_core::tiles::pmtiles::PmtCache::new(0)
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
            Some(martin_core::sprites::SpriteCache::new(size))
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
            Some(martin_core::fonts::FontCache::new(size))
        } else {
            tracing::info!("Font caching is disabled");
            None
        }
    }
}

/// Settings for one cache
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedSubCacheSetting {
    /// Maximum size for cache in Bytes
    pub size_mb: NonZeroU64,
}

impl ResolvedSubCacheSetting {
    pub fn new_opt(size_bytes: u64) -> Option<Self> {
        let size_mb = size_bytes / 1000 / 1000;
        let size = NonZeroU64::try_from(size_mb).ok();
        size.map(|size_mb| Self { size_mb })
    }
}

/// The cache configuration used in the configuration file
///
/// This is different from [`ResolvedCacheConfig`] as this still contains the override logic and not just the final values
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde_with::skip_serializing_none]
pub struct CacheConfig {
    /// Amount of memory (in Bytes) to use for caching [default: 512, 0 to disable]
    ///
    /// This is the total amount of cache we use.
    /// By default, this is split up between:
    /// - Tiles 50% -> 256 MB
    /// - Pmtiles' directories 25% -> 128 MB
    /// - Fonts 12.5% -> 64 MB
    /// - Sprites 12.5% -> 64 MB
    ///
    /// How the cache works internally is unstable and may change to improve performance/efficiency.
    /// For example, we may change the split between sources to improve efficiency.
    #[serde(
        serialize_with = "serialize_bytes",
        deserialize_with = "deserialize_bytes",
        default
    )]
    pub size: Option<u64>,

    #[serde(default)]
    pub tiles: SubCacheSetting,
    #[serde(default)]
    pub pmtile_directorys: SubCacheSetting,
    #[serde(default)]
    pub sprites: SubCacheSetting,
    #[serde(default)]
    pub fonts: SubCacheSetting,
}

impl From<CacheConfig> for ResolvedCacheConfig {
    fn from(config: CacheConfig) -> Self {
        if let Some(cache_size_bytes) = config.size {
            // Default: 50% for tiles
            #[cfg(feature = "_tiles")]
            let tile_cache_size_bytes = config.tiles.size.unwrap_or(cache_size_bytes / 2);

            // Default: 25% for PMTiles directories;
            #[cfg(feature = "pmtiles")]
            let pmtiles_cache_size_bytes = config
                .pmtile_directorys
                .size
                .unwrap_or(cache_size_bytes / 4);

            // Default: 12.5% for sprites
            #[cfg(feature = "sprites")]
            let sprite_cache_size_bytes = config.sprites.size.unwrap_or(cache_size_bytes / 8);

            // Default: 12.5% for fonts
            #[cfg(feature = "fonts")]
            let font_cache_size_bytes = config.fonts.size.unwrap_or(cache_size_bytes / 8);

            ResolvedCacheConfig {
                #[cfg(feature = "_tiles")]
                tiles: ResolvedSubCacheSetting::new_opt(tile_cache_size_bytes),
                #[cfg(feature = "pmtiles")]
                pmtile_directorys: ResolvedSubCacheSetting::new_opt(pmtiles_cache_size_bytes),
                #[cfg(feature = "sprites")]
                sprites: ResolvedSubCacheSetting::new_opt(sprite_cache_size_bytes),
                #[cfg(feature = "fonts")]
                fonts: ResolvedSubCacheSetting::new_opt(font_cache_size_bytes),
            }
        } else {
            // TODO: the defaults could be smarter. If I don't have pmtiles sources, don't reserve cache for it
            ResolvedCacheConfig {
                #[cfg(feature = "_tiles")]
                tiles: ResolvedSubCacheSetting::new_opt(256),
                #[cfg(feature = "pmtiles")]
                pmtile_directorys: ResolvedSubCacheSetting::new_opt(128),
                #[cfg(feature = "sprites")]
                sprites: ResolvedSubCacheSetting::new_opt(64),
                #[cfg(feature = "fonts")]
                fonts: ResolvedSubCacheSetting::new_opt(64),
            }
        }
    }
}

/// Settings for one cache
#[derive(Deserialize, Serialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
#[serde_with::skip_serializing_none]
pub struct SubCacheSetting {
    /// Maximum size for cache in Bytes
    #[serde(
        serialize_with = "serialize_bytes",
        deserialize_with = "deserialize_bytes"
    )]
    pub size: Option<u64>,
}

fn serialize_bytes<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(size) => serializer.serialize_str(&format!("{size}B")),
        None => serializer.serialize_none(),
    }
}
fn deserialize_bytes<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let bytes = s
                .parse::<humanize_rs::bytes::Bytes>()
                .map_err(|e| serde::de::Error::custom(format!("invalid byte size '{s}': {e}")))?;

            Ok(Some(bytes.size() as u64))
        }
        None => Ok(None),
    }
}
