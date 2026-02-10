use std::num::NonZeroU64;

use martin_core::config::OptBoolObj;
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
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CacheConfig(OptBoolObj<InnerCacheConfig>);

impl CacheConfig {
    pub fn set_size(&mut self, size_bytes: u64) {
        match (size_bytes, &mut self.0) {
            (0, i) => {
                *i = OptBoolObj::Bool(false);
            }
            (size_bytes, &mut OptBoolObj::Object(ref mut c)) => {
                c.size = Some(size_bytes);
            }
            (size_bytes, i) => {
                *i = OptBoolObj::Object(InnerCacheConfig::new(size_bytes));
            }
        };
    }

    /// Returns a mutable reference to the inner cache configuration
    ///
    /// This may enable the cache if it was disabled before
    pub fn object_mut(&mut self) -> &mut InnerCacheConfig {
        if !matches!(self.0, OptBoolObj::Object(_)) {
            *self = Self(OptBoolObj::Object(InnerCacheConfig::default()));
        }
        match self.0 {
            OptBoolObj::Object(ref mut c) => c,
            _ => unreachable!(),
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self.0, OptBoolObj::NoValue)
    }
}

impl From<CacheConfig> for ResolvedCacheConfig {
    fn from(config: CacheConfig) -> Self {
        let inner_cfg = match config.0 {
            OptBoolObj::NoValue => InnerCacheConfig::default(),
            OptBoolObj::Bool(true) => InnerCacheConfig::default(),
            OptBoolObj::Bool(false) => InnerCacheConfig {
                size: Some(0),
                tiles: SubCacheSetting::default(),
                pmtile_directorys: SubCacheSetting::default(),
                sprites: SubCacheSetting::default(),
                fonts: SubCacheSetting::default(),
            },
            OptBoolObj::Object(c) => c,
        };
        ResolvedCacheConfig::from(inner_cfg)
    }
}

/// Cache configuration if the user has enabled it
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InnerCacheConfig {
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
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub size: Option<u64>,

    #[serde(default, skip_serializing_if = "SubCacheSetting::is_none")]
    pub tiles: SubCacheSetting,
    #[serde(default, skip_serializing_if = "SubCacheSetting::is_none")]
    pub pmtile_directorys: SubCacheSetting,
    #[serde(default, skip_serializing_if = "SubCacheSetting::is_none")]
    pub sprites: SubCacheSetting,
    #[serde(default, skip_serializing_if = "SubCacheSetting::is_none")]
    pub fonts: SubCacheSetting,
}

impl InnerCacheConfig {
    pub fn new(size_bytes: u64) -> Self {
        Self {
            size: Some(size_bytes),
            tiles: SubCacheSetting::default(),
            pmtile_directorys: SubCacheSetting::default(),
            sprites: SubCacheSetting::default(),
            fonts: SubCacheSetting::default(),
        }
    }
}

impl From<InnerCacheConfig> for ResolvedCacheConfig {
    fn from(config: InnerCacheConfig) -> Self {
        let cache_size_bytes = config.size.unwrap_or(512 * 1000 * 1000);

        // TODO: the defaults could be smarter. If I don't have pmtiles sources, don't reserve cache for it
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
    }
}

/// Settings for one cache
#[derive(Deserialize, Serialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubCacheSetting {
    /// Maximum size for cache in Bytes
    #[serde(
        serialize_with = "serialize_bytes",
        deserialize_with = "deserialize_bytes"
    )]
    pub size: Option<u64>,
}

impl SubCacheSetting {
    fn is_none(&self) -> bool {
        self.size.is_none()
    }
}

fn serialize_bytes<S>(value: &Option<u64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(size) => {
            let size = size_format::SizeFormatterSI::new(*size);
            serializer.serialize_str(&format!("{size}B"))
        }
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
