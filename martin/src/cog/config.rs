use std::fmt::Debug;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use super::source::CogSource;
use crate::Source;
use crate::config::UnrecognizedValues;
use crate::file_config::{ConfigExtras, FileResult, SourceConfigExtras};

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
    async fn new_sources(&self, id: String, path: PathBuf) -> FileResult<Box<dyn Source>> {
        let cog = CogSource::new(id, path)?;
        Ok(Box::new(cog))
    }

    #[allow(clippy::no_effect_underscore_binding)]
    async fn new_sources_url(&self, _id: String, _url: Url) -> FileResult<Box<dyn Source>> {
        unreachable!()
    }

    fn parse_urls() -> bool {
        false
    }
}
