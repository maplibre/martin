
use std::ops::Deref;
use std::path::PathBuf;

use martin_core::config::OptOneMany;
use martin_core::fonts::FontSources;
use serde::{Deserialize, Serialize};

use crate::config::file::{ConfigFileError, ConfigFileResult};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FontConfig(OptOneMany<PathBuf>);

impl FontConfig {
    /// Discovers and loads fonts from the specified directories by recursively scanning for `.ttf`, `.otf`, and `.ttc` files.
    pub fn resolve(&mut self) -> ConfigFileResult<FontSources> {
        let mut results = FontSources::default();

        for path in self.iter() {
            results
                .recursively_add_directory(path.clone())
                .map_err(|e| ConfigFileError::FontResolutionFailed(e, path.clone()))?;
        }

        Ok(results)
    }

    pub fn new(font: impl IntoIterator<Item = PathBuf>) -> Self {
        Self(OptOneMany::new(font))
    }
}

impl Deref for FontConfig {
    type Target = OptOneMany<PathBuf>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
