use std::fmt::Debug;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::cog::CogSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{
    ConfigurationLivecycleHooks, TileSourceConfiguration, UnrecognizedKeys, UnrecognizedValues,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CogConfig {
    /// Default true
    ///
    /// |option |Reprojecting to WebMercatorQuad|Pro And Con|
    /// |---|---|
    /// |true|Server Side|1. A little bit slow as Martin would do some cliping and merging</br>2. No any extra configuration needed for map viewers as WebMercatorQuad is the most default|
    /// |false|Client Side|1. Most efficient 2. Need extra configuration for map viewers|
    ///
    ///
    /// As martin currently has no support for CRS not 3857, we strongly recommend to enable this option
    pub auto_webmercator: Option<bool>,

    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for CogConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl TileSourceConfiguration for CogConfig {
    fn parse_urls() -> bool {
        false
    }

    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<BoxedSource> {
        let cog = CogSource::new(id, path)?;
        Ok(Box::new(cog))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<BoxedSource> {
        unreachable!()
    }
}
