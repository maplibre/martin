use std::fmt::Debug;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::geojson::source::GeoJsonSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{
    ConfigurationLivecycleHooks, TileSourceConfiguration, UnrecognizedKeys, UnrecognizedValues,
};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GeoJsonConfig {
    #[serde(flatten, skip_serializing)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigurationLivecycleHooks for GeoJsonConfig {
    fn get_unrecognized_keys(&self) -> UnrecognizedKeys {
        self.unrecognized.keys().cloned().collect()
    }
}

impl TileSourceConfiguration for GeoJsonConfig {
    fn parse_urls() -> bool {
        false
    }

    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<BoxedSource> {
        let geojson_source = GeoJsonSource::new(id, path).await?;
        Ok(Box::new(geojson_source))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<BoxedSource> {
        unreachable!()
    }
}
