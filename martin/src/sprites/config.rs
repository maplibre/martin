use serde::{Deserialize, Serialize};

use crate::config::UnrecognizedValues;
use crate::file_config::ConfigExtras;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpriteConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for SpriteConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}
