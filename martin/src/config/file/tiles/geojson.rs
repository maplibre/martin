use std::fmt::Debug;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::geojson::GeoJsonSource;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::MartinResult;
use crate::config::file::{
    ConfigurationLivecycleHooks, TileSourceConfiguration, UnrecognizedKeys, UnrecognizedValues,
};

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
    /// maximum zoom
    #[serde(default = "GeoJsonConfig::default_max_zoom")]
    pub max_zoom: u8,
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
            max_zoom: Self::default_max_zoom(),
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

    fn default_max_zoom() -> u8 {
        geojson_vt_rs::Options::default().max_zoom
    }
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
        let tile_options = geojson_vt_rs::TileOptions {
            tolerance: self.tolerance,
            extent: self.extent,
            buffer: self.buffer,
            line_metrics: self.line_metrics,
        };
        Ok(Box::new(GeoJsonSource::new(
            id,
            path,
            self.max_zoom,
            tile_options,
        )?))
    }

    async fn new_sources_url(&self, _id: String, _url: Url) -> MartinResult<BoxedSource> {
        unreachable!()
    }
}
