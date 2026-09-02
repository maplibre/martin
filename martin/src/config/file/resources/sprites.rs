use std::collections::BTreeMap;
use std::path::PathBuf;

use martin_core::sprites::SpriteSources;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::file::resources::subdirectories;
use crate::config::file::{
    CacheSizeConfig, CollectUnrecognizedKeys, ConfigFileError, ConfigFileResult,
    ConfigurationLivecycleHooks, FileConfigEnum, UnrecognizedValues,
};
use crate::config::primitives::OptOneMany;

pub type SpriteConfig = FileConfigEnum<InnerSpriteConfig>;
impl SpriteConfig {
    pub fn resolve(&mut self) -> ConfigFileResult<SpriteSources> {
        let Some(cfg) = self.extract_file_config() else {
            return Ok(SpriteSources::default());
        };

        let results = SpriteSources::default();
        let mut directories = Vec::new();
        let mut configs = BTreeMap::new();

        if let Some(sources) = cfg.sources {
            for (id, source) in sources {
                configs.insert(id.clone(), source.clone());
                results.add_source(id, source.abs_path()?);
            }
        }

        for path in cfg.paths {
            let Some(name) = path.file_name() else {
                warn!(
                    "Ignoring sprite source with no name from {}",
                    path.display()
                );
                continue;
            };
            directories.push(path.clone());
            results.add_source(name.to_string_lossy().to_string(), path);
        }

        for root in cfg.custom.collections.iter() {
            for (name, path) in subdirectories(root)? {
                results.add_source(name, path);
            }
        }

        for (alias, sprites) in &cfg.custom.aliases {
            results
                .add_alias(alias.clone(), sprites.clone())
                .map_err(ConfigFileError::SpriteAliasResolutionFailed)?;
        }

        *self = Self::new_extended(directories, configs, cfg.custom);

        Ok(results)
    }
}

#[serde_with::skip_serializing_none]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, CollectUnrecognizedKeys)]
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

    /// Directories whose subdirectories are each published as a sprite source named after them.
    #[serde(default, skip_serializing_if = "OptOneMany::is_none")]
    pub collections: OptOneMany<PathBuf>,

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

impl ConfigurationLivecycleHooks for InnerSpriteConfig {
    fn has_content(&self) -> bool {
        !self.collections.is_none()
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[test]
    fn sprites_parse_collections() {
        let yaml = indoc! {"
            collections:
              - /projects/sprites
        "};
        let cfg: SpriteConfig =
            serde_saphyr::from_str(yaml).expect("sprites with collections must parse");
        let SpriteConfig::Config(cfg) = cfg else {
            panic!("expected Config variant, got {cfg:?}");
        };
        assert_eq!(
            cfg.custom.collections.iter().collect::<Vec<_>>(),
            vec![&PathBuf::from("/projects/sprites")]
        );
    }
}
