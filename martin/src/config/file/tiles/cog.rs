use std::fmt::Debug;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::cog::CogSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{
    CachePolicy, ConfigurationLivecycleHooks, TileSourceConfiguration, UnrecognizedKeys,
    UnrecognizedValues,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
pub struct CogConfig {
    #[serde(flatten, skip_serializing)]
    #[cfg_attr(feature = "unstable-schemas", schemars(skip))]
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

    async fn new_sources(
        &self,
        id: String,
        path: PathBuf,
        cache: CachePolicy,
    ) -> MartinResult<BoxedSource> {
        let cog = CogSource::new(id, path, cache.zoom())?;
        Ok(Box::new(cog))
    }

    async fn new_sources_url(
        &self,
        _id: String,
        _url: Url,
        _cache: CachePolicy,
    ) -> MartinResult<BoxedSource> {
        unreachable!()
    }
}
