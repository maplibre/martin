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

    /// Default false
    /// If enabled, martin will automatically serve COG as a [WebMercatorQuad](https://docs.ogc.org/is/17-083r2/17-083r2.html#72) service, the tiles will be cliped and merged internally to be aligned with the Web Mercator grid.
    /// Note: Just work for COG files with a Web Mercator CRS (EPSG:3857).
    pub auto_web: Option<bool>,
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

    async fn new_sources(&self, id: String, path: PathBuf) -> FileResult<Box<dyn Source>> {
        let cog = CogSource::new(id, path, self.auto_web.unwrap_or(false))?;
        Ok(Box::new(cog))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> FileResult<Box<dyn Source>> {
        unreachable!()
    }
}
