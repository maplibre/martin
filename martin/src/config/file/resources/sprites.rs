use std::collections::BTreeMap;

use martin_core::sprites::SpriteSources;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::file::{
    CacheSizeConfig, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    ConfigurationLivecycleHooks, FileConfig, UnrecognizedValues, subdirectories,
};

pub type SpriteConfig = FileConfig<InnerSpriteConfig>;
impl SpriteConfig {
    pub fn resolve(&mut self) -> ConfigFileResult<SpriteSources> {
        let results = SpriteSources::default();

        for (id, source) in &self.sources {
            results.add_source(id.clone(), source.abs_path()?);
        }

        self.paths.retain(|path| {
            let Some(name) = path.file_name() else {
                warn!(
                    "Ignoring sprite source with no name from {}",
                    path.display()
                );
                return false;
            };
            results.add_source(name.to_string_lossy().to_string(), path.clone());
            true
        });

        for collection in &self.collections {
            for (name, path) in subdirectories(collection)
                .map_err(|e| ConfigFileError::IoError(e, collection.clone()))?
            {
                results.add_source(name, path);
            }
        }

        for (alias, sprites) in &self.custom.aliases {
            results
                .add_alias(alias.clone(), sprites.clone())
                .map_err(ConfigFileError::SpriteAliasResolutionFailed)?;
        }

        Ok(results)
    }
}

#[serde_with::skip_serializing_none]
#[derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    CollectUnrecognizedKeys,
    ConfigurationLivecycleHooks,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct InnerSpriteConfig {
    /// Cache configuration for sprites.
    /// Use `cache: disable` to disable sprite caching.
    #[serde(default, skip_serializing_if = "CacheSizeConfig::is_empty")]
    #[cfg_attr(
        feature = "unstable-schemas",
        schemars(with = "crate::config::file::CacheSizeConfigShape")
    )]
    pub cache: CacheSizeConfig,

    /// Named combinations of sprite sources.
    ///
    /// Each alias can be requested like a sprite source and serves the listed sources combined.
    /// Aliases may only reference configured sprite sources, not other aliases.
    /// An alias sharing the name of a sprite source takes precedence over it.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, Vec<String>>,

    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
    pub unrecognized: UnrecognizedValues,
}
