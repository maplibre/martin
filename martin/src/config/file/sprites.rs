use std::collections::BTreeMap;

use log::warn;
use martin_core::sprites::SpriteSources;
use serde::{Deserialize, Serialize};

use crate::config::file::{
    ConfigFileResult, ConfigurationLivecycleHooks, FileConfigEnum, UnrecognizedKeys,
    UnrecognizedValues,
};

pub type SpriteConfig = FileConfigEnum<InnerSpriteConfig>;
impl SpriteConfig {
    pub fn resolve(&mut self) -> ConfigFileResult<SpriteSources> {
        let Some(cfg) = self.extract_file_config()? else {
            return Ok(SpriteSources::default());
        };

        let mut results = SpriteSources::default();
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

        *self = FileConfigEnum::new_extended(directories, configs, cfg.custom);

        Ok(results)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InnerSpriteConfig {
    /// Size of the sprite cache in megabytes (0 to disable)
    ///
    /// Overrides [`cache_size_mb`](crate::config::file::Config::cache_size_mb).
    pub cache_size_mb: Option<u64>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for InnerSpriteConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}
