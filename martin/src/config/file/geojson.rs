use std::fmt::Debug;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{ConfigExtras, SourceConfigExtras, UnrecognizedValues};
use crate::geojson::GeoJsonSource;
use crate::source::TileInfoSource;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GeoJsonConfig {
    /// simplification tolerance (higher means simpler)
    #[serde(default = "GeoJsonConfig::default_tolerance")]
    pub tolerance: f64,
    /// tile extent
    #[serde(default = "GeoJsonConfig::default_extent")]
    pub extent: u16,
    /// tile buffer on each side (default 64)
    #[serde(default = "GeoJsonConfig::default_buffer")]
    pub buffer: u16,
    /// enable line metrics tracking for LineString/MultiLineString features
    #[serde(default = "GeoJsonConfig::default_line_metrics")]
    pub line_metrics: bool,
    #[serde(flatten)]
    pub unrecognized: UnrecognizedValues,
}

impl Default for GeoJsonConfig {
    fn default() -> Self {
        Self {
            tolerance: Self::default_tolerance(),
            extent: Self::default_extent(),
            buffer: Self::default_buffer(),
            line_metrics: Self::default_line_metrics(),
            unrecognized: Default::default(),
        }
    }
}

impl GeoJsonConfig {
    fn default_tolerance() -> f64 {
        geojson_vt_rs::TileOptions::default().tolerance
    }

    fn default_extent() -> u16 {
        geojson_vt_rs::TileOptions::default().extent
    }

    fn default_buffer() -> u16 {
        geojson_vt_rs::TileOptions::default().buffer
    }

    fn default_line_metrics() -> bool {
        geojson_vt_rs::TileOptions::default().line_metrics
    }
}

impl ConfigExtras for GeoJsonConfig {
    fn get_unrecognized(&self) -> &UnrecognizedValues {
        &self.unrecognized
    }
}

impl SourceConfigExtras for GeoJsonConfig {
    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<TileInfoSource> {
        let tile_options = geojson_vt_rs::TileOptions {
            tolerance: self.tolerance,
            extent: self.extent,
            buffer: self.buffer,
            line_metrics: self.line_metrics,
        };
        Ok(Box::new(GeoJsonSource::new(id, path, tile_options)?))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<TileInfoSource> {
        unreachable!()
    }
}
