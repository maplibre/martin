use std::fmt::{Debug, Formatter};
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use geojson_vt_rs::{GeoJSONVT, Options};
use geozero::mvt::Message as _;
use martin_tile_utils::{Format, TileCoord, TileInfo};
use std::fs::File;
use tilejson::TileJSON;
use tilejson::tilejson;
use tokio::sync::RwLock;

use crate::file_config::FileError;
use crate::file_config::FileResult;
use crate::geojson::mvt::LayerBuilder;
use crate::source::{TileData, TileInfoSource, UrlQuery};
use crate::{MartinResult, Source};

mod config;
mod mvt;

pub use config::GeoJsonConfig;

pub struct GeoJsonSource {
    id: String,
    path: PathBuf,
    inner: Arc<RwLock<GeoJSONVT>>,
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

        // TODO: see if options default is goood enough
        let inner = GeoJSONVT::from_geojson(&geojson, &Options::default());

        let tilejson = tilejson! {
            tiles: vec![],
            minzoom: 0,
            maxzoom: 18,
        };

        return Ok(Self {
            id,
            path,
            inner: Arc::new(RwLock::new(inner)),
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
        // TODO: implement clone for GeoJsonSource (GeoJSONVT is not cloneable, which is a blocker)
        Box::new(Self::new(self.id.clone(), self.path.clone()).unwrap())
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
        let tile;
        {
            let mut guard = self.inner.write().await;
            tile = guard.get_tile(xyz.z, xyz.x, xyz.y).clone();
        }
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
