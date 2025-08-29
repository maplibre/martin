use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use geozero::mvt::Message as _;
use martin_tile_utils::{Format, TileCoord, TileInfo};
use std::fs::File;
use tilejson::TileJSON;
use tilejson::tilejson;

use crate::file_config::FileError;
use crate::file_config::FileResult;
use crate::geojson::mvt::LayerBuilder;
use crate::source::{TileData, TileInfoSource, UrlQuery};
use crate::{MartinResult, Source};

mod config;
mod mvt;

pub use config::GeoJsonConfig;

#[derive(Clone)]
pub struct GeoJsonSource {
    id: String,
    path: PathBuf,
    geojson: Arc<geojson::GeoJson>,
    tilejson: TileJSON,
    tile_info: TileInfo,
}

impl Debug for GeoJsonSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeoJsonSource")
            .field("id", &self.id)
            .field("path", &self.path)
            .finish()
    }
}

impl GeoJsonSource {
    fn new(id: String, path: PathBuf) -> FileResult<Self> {
        let tile_info = TileInfo::new(Format::Mvt, martin_tile_utils::Encoding::Uncompressed);
        let geojson_file =
            File::open(&path).map_err(|e: std::io::Error| FileError::IoError(e, path.clone()))?;

        // TODO: better error handling
        let geojson = geojson::GeoJson::from_reader(geojson_file)
            .map_err(|e| FileError::InvalidFilePath(path.clone()))?;

        // TODO: vector layers
        let tilejson = tilejson! {
            tiles: vec![],
            vector_layers: geojson_to_vector_layer(&id, &geojson),
            minzoom: 0,
            maxzoom: 18,
        };

        return Ok(Self {
            id,
            path,
            geojson: Arc::new(geojson),
            tilejson,
            tile_info,
        });
    }
}

#[async_trait]
impl Source for GeoJsonSource {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> TileInfoSource {
        Box::new(self.clone())
    }

    fn benefits_from_concurrent_scraping(&self) -> bool {
        // TODO: figure out if this is a good idea
        false
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinResult<TileData> {
        // TODO: get from source (self)
        let options = geojson_vt_rs::TileOptions::default();
        let tile = geojson_vt_rs::geojson_to_tile(
            &self.geojson,
            xyz.z,
            xyz.x,
            xyz.y,
            &options,
            true,
            true,
        );
        let mut builder = LayerBuilder::new(self.id.clone(), 4096);
        for feature in &tile.features.features {
            builder.add_feature(feature);
        }
        let mvt_tile = geozero::mvt::Tile {
            layers: vec![builder.build()],
        };
        Ok(mvt_tile.encode_to_vec())
    }
}

// TODO: maybe break down fields subroutine into a function
fn geojson_to_vector_layer(
    layer_name: &str,
    geojson: &geojson::GeoJson,
) -> Vec<tilejson::VectorLayer> {
    let mut fields = BTreeMap::new();
    match geojson {
        geojson::GeoJson::Geometry(_geometry) => {
            vec![tilejson::VectorLayer::new(layer_name.to_string(), fields)]
        }
        geojson::GeoJson::Feature(feature) => {
            if let Some(properties) = feature.properties.as_ref() {
                properties.iter().for_each(|(key, value)| {
                    // should be in sync with mvt::tilevalue_from_json
                    let val_type = match value {
                        serde_json::Value::String(_) => "string",
                        serde_json::Value::Number(n) => {
                            if n.is_f64() {
                                "double"
                            } else if n.is_i64() {
                                "signed integer"
                            } else if n.is_u64() {
                                "unsigned integer"
                            } else {
                                // TODO: check
                                unreachable!()
                            }
                        }
                        serde_json::Value::Bool(_) => "boolean",
                        _ => "string",
                    };
                    fields.insert(key.to_string(), val_type.to_string());
                });
            }
            vec![tilejson::VectorLayer::new(layer_name.to_string(), fields)]
        }
        geojson::GeoJson::FeatureCollection(feature_collection) => {
            for feature in &feature_collection.features {
                if let Some(properties) = feature.properties.as_ref() {
                    properties.iter().for_each(|(key, value)| {
                        // should be in sync with mvt::tilevalue_from_json
                        let val_type = match value {
                            serde_json::Value::String(_) => "string",
                            serde_json::Value::Number(n) => {
                                if n.is_f64() {
                                    "double"
                                } else if n.is_i64() {
                                    "signed integer"
                                } else if n.is_u64() {
                                    "unsigned integer"
                                } else {
                                    // TODO: check
                                    unreachable!()
                                }
                            }
                            serde_json::Value::Bool(_) => "boolean",
                            _ => "string",
                        };
                        fields.insert(key.to_string(), val_type.to_string());
                    });
                }
            }
            vec![tilejson::VectorLayer::new(layer_name.to_string(), fields)]
        }
    }
}
