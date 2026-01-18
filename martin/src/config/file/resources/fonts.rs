use std::collections::BTreeMap;
use std::time::Duration;

use martin_core::fonts::FontSources;
use serde::{Deserialize, Serialize};

use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, FileConfigEnum,
    UnrecognizedKeys, UnrecognizedValues,
};

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InnerFontConfig {
    /// Size of the font cache in megabytes (0 to disable)
    ///
    /// Overrides [`cache_size_mb`](crate::config::file::Config::cache_size_mb).
    pub cache_size_mb: Option<u64>,

    /// Maximum lifetime for cached fonts (TTL - time to live from creation).
    /// Supports human-readable formats like "1h", "30m", "1d", or "3600s".
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub font_cache_expiry: Option<Duration>,

    /// Maximum idle time for cached fonts (TTI - time to idle since last access).
    /// Supports human-readable formats like "5m", "300s", or "1h".
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    pub font_cache_idle_timeout: Option<Duration>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}
impl ConfigurationLivecycleHooks for InnerFontConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

pub type FontConfig = FileConfigEnum<InnerFontConfig>;

impl FontConfig {
    /// Discovers and loads fonts from the specified directories by recursively scanning for `.ttf`, `.otf`, and `.ttc` files.
    pub fn resolve(&mut self) -> ConfigFileResult<FontSources> {
        let Some(cfg) = self.extract_file_config() else {
            return Ok(FontSources::default());
        };

        let mut results = FontSources::default();
        let mut directories = Vec::new();
        let mut configs = BTreeMap::new();

        if let Some(sources) = cfg.sources {
            for (id, source) in sources {
                configs.insert(id.clone(), source.clone());
                results
                    .recursively_add_directory(source.get_path().clone())
                    .map_err(|e| ConfigFileError::FontResolutionFailed(e, source.into_path()))?;
            }
        }

        for base_path in cfg.paths {
            directories.push(base_path.clone());
            results
                .recursively_add_directory(base_path.clone())
                .map_err(|e| ConfigFileError::FontResolutionFailed(e, base_path.clone()))?;
        }

        *self = FileConfigEnum::new_extended(directories, configs, cfg.custom);

        Ok(results)
    }
}
