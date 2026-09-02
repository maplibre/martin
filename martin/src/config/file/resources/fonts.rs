use std::collections::BTreeMap;

use martin_core::fonts::FontSources;
use serde::{Deserialize, Serialize};

use crate::config::file::{
    CacheSizeConfig, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    ConfigurationLivecycleHooks, FileConfigEnum, UnrecognizedValues,
};

#[serde_with::skip_serializing_none]
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct InnerFontConfig {
    /// Cache configuration for fonts.
    /// Use `cache: disable` to disable font caching.
    #[serde(default, skip_serializing_if = "CacheSizeConfig::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CacheSizeConfigShape")
    )]
    pub cache: CacheSizeConfig,

    /// Named font stacks.
    ///
    /// Each alias can be requested like a font and serves the listed fonts combined, in fallback order.
    /// Aliases may only reference discovered fonts, not other aliases.
    /// An alias sharing the name of a discovered font takes precedence over it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, Vec<String>>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
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

        for (alias, fonts) in &cfg.custom.aliases {
            results
                .add_alias(alias.clone(), fonts.clone())
                .map_err(ConfigFileError::FontAliasResolutionFailed)?;
        }

        *self = Self::new_extended(directories, configs, cfg.custom);

        Ok(results)
    }
}
