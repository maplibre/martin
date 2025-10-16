use martin_core::fonts::FontSources;
use serde::{Deserialize, Serialize};

use crate::config::file::{
    ConfigFileError, ConfigFileResult, ConfigurationLivecycleHooks, FileConfigEnum,
    UnrecognizedKeys, UnrecognizedValues,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InnerFontConfig {
    /// Size of the font cache in megabytes (0 to disable)
    ///
    /// Overrides [`cache_size_mb`](crate::config::file::Config::cache_size_mb).
    pub cache_size_mb: Option<u64>,

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
        // TODO: make sure that the resolution is similarly tolerant of errors as the other resolution methods (warning instead of hard errors)

        if let Some(sources) = cfg.sources {
            for (_id, source) in sources {
                results
                    .recursively_add_directory(source.get_path().clone())
                    .map_err(|e| ConfigFileError::FontResolutionFailed(e, source.into_path()))?;
            }
        }

        for base_path in cfg.paths {
            results
                .recursively_add_directory(base_path.clone())
                .map_err(|e| ConfigFileError::FontResolutionFailed(e, base_path.clone()))?;
        }

        Ok(results)
    }
}
