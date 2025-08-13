use serde::{Deserialize, Serialize};

use crate::config::{UnrecognizedKeys, UnrecognizedValues};
use crate::file_config::ConfigExtras;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpriteConfig {
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for SpriteConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}
