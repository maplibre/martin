use std::fmt::Debug;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::cog::CogSource;
use crate::config::file::{ConfigExtras, SourceConfigExtras, UnrecognizedValues};
use crate::{MartinResult, TileInfoSource};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CogConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for CogConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

impl SourceConfigExtras for CogConfig {
    fn parse_urls() -> bool {
        false
    }

    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<TileInfoSource> {
        let cog = CogSource::new(id, path)?;
        Ok(Box::new(cog))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<TileInfoSource> {
        unreachable!()
    }
}
