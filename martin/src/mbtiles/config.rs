use std::fmt::Debug;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::{UnrecognizedKeys, UnrecognizedValues};
use crate::file_config::{ConfigExtras, SourceConfigExtras};
use crate::mbtiles::MbtSource;
use crate::source::TileInfoSource;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MbtConfig {
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for MbtConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl SourceConfigExtras for MbtConfig {
    fn parse_urls() -> bool {
        false
    }
    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<TileInfoSource> {
        Ok(Box::new(MbtSource::new(id, path).await?))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<TileInfoSource> {
        unreachable!()
    }
}
