//! `GeoJSON` tile source implementation.

use std::fmt::{Debug, Formatter};
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use super::utils;
use async_trait::async_trait;
use geozero::FeatureProcessor;
use geozero::mvt::{Message as _, MvtWriter};
use martin_tile_utils::{Format, TileCoord, TileData, TileInfo};
use tilejson::{TileJSON, tilejson};

use super::GeoJsonError;
use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

/// Tile source that reads from `GeoJSON` files.
#[derive(Clone)]
pub struct GeoJsonSource {
    id: String,
    path: PathBuf,
    extent: u16,
    preprocessed: Arc<geojson_vt_rs::PreprocessedGeoJSON>,
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
        max_zoom: u8,
        tile_options: geojson_vt_rs::TileOptions,
    ) -> Result<Self, GeoJsonError> {
        let tile_info = TileInfo::new(Format::Mvt, martin_tile_utils::Encoding::Uncompressed);
        let geojson_file = File::open(&path)
            .map_err(|e: std::io::Error| GeoJsonError::IoError(e, path.clone()))?;

        let geojson = geojson::GeoJson::from_reader(geojson_file)
            .map_err(|e| GeoJsonError::NotValidGeoJson(e, path.clone()))?;

        let preprocessed =
            geojson_vt_rs::PreprocessedGeoJSON::new(&geojson, max_zoom, &tile_options);

        let bounds = match &geojson {
            geojson::GeoJson::Geometry(geometry) => utils::geojson_to_bounds(&geometry.value),
            geojson::GeoJson::Feature(feature) => match feature.geometry.as_ref() {
                Some(geom) => utils::geojson_to_bounds(&geom.value),
                None => return Err(GeoJsonError::NoGeometry(path.clone())),
            },
            geojson::GeoJson::FeatureCollection(feature_collection) => {
                let mut bounds = tilejson::Bounds::new(f64::MAX, f64::MAX, f64::MIN, f64::MIN);
                for feature in &feature_collection.features {
                    match feature.geometry.as_ref() {
                        Some(geom) => {
                            let feat_bounds = utils::geojson_to_bounds(&geom.value);
                            utils::update_bounds(&mut bounds, feat_bounds.left, feat_bounds.bottom);
                            utils::update_bounds(&mut bounds, feat_bounds.right, feat_bounds.top);
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
            vector_layers: utils::geojson_to_vector_layer(&id, &geojson),
            bounds: bounds,
            minzoom: 0,
            maxzoom: max_zoom, // from geojson-vt-rs max_zoom
        };

        return Ok(Self {
            id,
            path,
            extent: tile_options.extent,
            preprocessed: Arc::new(preprocessed),
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
        let tile = self.preprocessed.generate_tile(xyz.z, xyz.x, xyz.y);

        let mut mvt_writer =
            MvtWriter::new_unscaled(self.extent as u32).map_err(GeoJsonError::from)?;

        let _ = mvt_writer.dataset_begin(None);
        for (idx, feature) in tile.features.features.iter().enumerate() {
            let idx = idx as u64;
            let _ = mvt_writer.feature_begin(idx);
            let _ = mvt_writer.properties_begin();
            if let Some(properties) = feature.properties.as_ref() {
                utils::write_geojson_properties(&mut mvt_writer, idx as usize, properties);
            }
            let _ = mvt_writer.properties_end();
            let _ = mvt_writer.geometry_begin();
            if let Some(geom) = feature.geometry.as_ref() {
                utils::write_geojson_geom(&mut mvt_writer, &geom.value);
            }
            let _ = mvt_writer.geometry_end();
            let _ = mvt_writer.feature_end(idx);
        }
        let _ = mvt_writer.dataset_end();

        let mvt_tile = geozero::mvt::Tile {
            layers: vec![mvt_writer.layer(&self.id)],
        };
        Ok(mvt_tile.encode_to_vec())
    }
}
