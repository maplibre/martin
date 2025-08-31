use std::fmt::Debug;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{ConfigExtras, SourceConfigExtras, UnrecognizedValues};
use crate::mbtiles::MbtSource;
use crate::source::TileInfoSource;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MbtConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for MbtConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

impl SourceConfigExtras for MbtConfig {
    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<TileInfoSource> {
        Ok(Box::new(MbtSource::new(id, path).await?))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<TileInfoSource> {
        unreachable!()
    }
}
