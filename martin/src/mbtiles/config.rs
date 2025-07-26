use std::fmt::Debug;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::config::UnrecognizedValues;
use crate::file_config::{ConfigExtras, FileResult, SourceConfigExtras};
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
    async fn new_sources(&self, id: String, path: PathBuf) -> FileResult<TileInfoSource> {
        Ok(Box::new(MbtSource::new(id, path).await?))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> FileResult<TileInfoSource> {
        unreachable!()
    }
}
