use std::fmt::Debug;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::UnrecognizedValues;
use crate::file_config::{ConfigExtras, SourceConfigExtras};
use crate::geojson::GeoJsonSource;
use crate::source::TileInfoSource;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GeoJsonConfig {
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl ConfigExtras for GeoJsonConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

impl SourceConfigExtras for GeoJsonConfig {
    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<TileInfoSource> {
        Ok(Box::new(GeoJsonSource::new(id, path)?))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<TileInfoSource> {
        unreachable!()
    }
}
