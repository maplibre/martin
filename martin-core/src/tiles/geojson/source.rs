//! `GeoJSON` tile source implementation.

use std::collections::BTreeMap;
use std::fmt::{Debug, Formatter};
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use geozero::mvt::Message as _;
use martin_tile_utils::{Format, TileCoord, TileData, TileInfo};
use tilejson::{TileJSON, tilejson};

use super::GeoJsonError;
use crate::tiles::geojson::mvt::LayerBuilder;
use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

/// Tile source that reads from `GeoJSON` files.
#[derive(Clone)]
pub struct GeoJsonSource {
    id: String,
    path: PathBuf,
    geojson: Arc<geojson::GeoJson>,
    tile_options: geojson_vt_rs::TileOptions,
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
    /// Creates a new [`GeoJsonSource`] from the given file path.
    pub fn new(
        id: String,
        path: PathBuf,
        tile_options: geojson_vt_rs::TileOptions,
    ) -> Result<Self, GeoJsonError> {
        let tile_info = TileInfo::new(Format::Mvt, martin_tile_utils::Encoding::Uncompressed);
        let geojson_file = File::open(&path)
            .map_err(|e: std::io::Error| GeoJsonError::IoError(e, path.clone()))?;

        let geojson = geojson::GeoJson::from_reader(geojson_file)
            .map_err(|e| GeoJsonError::NotValidGeoJson(e, path.clone()))?;

        let bounds = match &geojson {
            geojson::GeoJson::Geometry(geometry) => geojson_to_bounds(&geometry.value),
            geojson::GeoJson::Feature(feature) => match feature.geometry.as_ref() {
                Some(geom) => geojson_to_bounds(&geom.value),
                None => return Err(GeoJsonError::NoGeometry(path.clone())),
            },
            geojson::GeoJson::FeatureCollection(feature_collection) => {
                let mut bounds = tilejson::Bounds::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for feature in &feature_collection.features {
                    match feature.geometry.as_ref() {
                        Some(geom) => {
                            let feat_bounds = geojson_to_bounds(&geom.value);
                            update_bounds(&mut bounds, feat_bounds.left, feat_bounds.bottom);
                            update_bounds(&mut bounds, feat_bounds.right, feat_bounds.top);
                        }
                        None => continue,
                    }
                }
                // no feature has geom so return an error
                if bounds == tilejson::Bounds::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN) {
                    return Err(GeoJsonError::NoGeometry(path.clone()));
                }
                bounds
            }
        };

        let tilejson = tilejson! {
            tiles: vec![],
            vector_layers: geojson_to_vector_layer(&id, &geojson),
            bounds: bounds,
            minzoom: 0,
            maxzoom: 24, // from geojson-vt-rs max_zoom
        };

        return Ok(Self {
            id,
            path,
            geojson: Arc::new(geojson),
            tile_options,
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

    fn clone_source(&self) -> BoxedSource {
        Box::new(self.clone())
    }

    fn benefits_from_concurrent_scraping(&self) -> bool {
        true
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        // TODO: get from source (self)
        let tile = geojson_vt_rs::geojson_to_tile(
            &self.geojson,
            xyz.z,
            xyz.x,
            xyz.y,
            &self.tile_options,
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

fn update_bounds(bounds: &mut tilejson::Bounds, x: f64, y: f64) {
    bounds.left = f64::min(bounds.left, x);
    bounds.right = f64::max(bounds.right, x);
    bounds.bottom = f64::min(bounds.bottom, y);
    bounds.top = f64::max(bounds.top, y);
}

fn geojson_to_bounds(geojson_value: &geojson::Value) -> tilejson::Bounds {
    let mut bounds = tilejson::Bounds::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);

    match geojson_value {
        geojson::Value::Point(point) => {
            update_bounds(&mut bounds, point[0], point[1]);
        }
        geojson::Value::MultiPoint(points) => {
            points.iter().for_each(|point| {
                update_bounds(&mut bounds, point[0], point[1]);
            });
        }
        geojson::Value::LineString(points) => {
            points.iter().for_each(|point| {
                update_bounds(&mut bounds, point[0], point[1]);
            });
        }
        geojson::Value::MultiLineString(linestrings) => {
            linestrings.iter().for_each(|linestring| {
                linestring.iter().for_each(|point| {
                    update_bounds(&mut bounds, point[0], point[1]);
                });
            });
        }
        geojson::Value::Polygon(rings) => {
            rings.iter().for_each(|ring| {
                ring.iter().for_each(|point| {
                    update_bounds(&mut bounds, point[0], point[1]);
                });
            });
        }
        geojson::Value::MultiPolygon(polygons) => {
            polygons.iter().for_each(|polygon| {
                polygon.iter().for_each(|ring| {
                    ring.iter().for_each(|point| {
                        update_bounds(&mut bounds, point[0], point[1]);
                    });
                });
            });
        }
        geojson::Value::GeometryCollection(geometries) => {
            for geometry in geometries {
                let col_bounds = geojson_to_bounds(&geometry.value);
                update_bounds(&mut bounds, col_bounds.left, col_bounds.bottom);
                update_bounds(&mut bounds, col_bounds.right, col_bounds.top);
            }
        }
    }

    return bounds;
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
