use serde::{Deserialize, Serialize};

use crate::config::UnrecognizedValues;
use crate::file_config::ConfigExtras;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StyleConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for StyleConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}
