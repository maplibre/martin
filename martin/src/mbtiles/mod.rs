use std::fmt::{Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use log::debug;
use martin_mbtiles::MbtilesPool;
use martin_tile_utils::TileInfo;
use tilejson::TileJSON;

use crate::file_config::FileError;
use crate::file_config::FileError::{AquireConnError, InvalidMetadata, IoError};
use crate::source::{Tile, UrlQuery};
use crate::utils::is_valid_zoom;
use crate::{Error, Source, Xyz};

#[derive(Clone)]
pub struct MbtSource {
    id: String,
    mbtiles: Arc<MbtilesPool>,
    tilejson: TileJSON,
    tile_info: TileInfo,
}

impl Debug for MbtSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MbtSource {{ id: {}, path: {:?} }}",
            self.id,
            self.mbtiles.as_ref()
        )
    }
}

impl MbtSource {
    pub async fn new_box(id: String, path: PathBuf) -> Result<Box<dyn Source>, FileError> {
        Ok(Box::new(MbtSource::new(id, path).await?))
    }

    async fn new(id: String, path: PathBuf) -> Result<Self, FileError> {
        let mbt = MbtilesPool::new(&path)
            .await
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("{e:?}: Cannot open file {}", path.display()),
                )
            })
            .map_err(|e| IoError(e, path.clone()))?;

        let meta = mbt
            .get_metadata()
            .await
            .map_err(|e| InvalidMetadata(e.to_string(), path))?;

        Ok(Self {
            id,
            mbtiles: Arc::new(mbt),
            tilejson: meta.tilejson,
            tile_info: meta.tile_info,
        })
    }
}

#[async_trait]
impl Source for MbtSource {
    fn get_tilejson(&self) -> TileJSON {
        self.tilejson.clone()
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    fn is_valid_zoom(&self, zoom: u8) -> bool {
        is_valid_zoom(zoom, self.tilejson.minzoom, self.tilejson.maxzoom)
    }

    fn support_url_query(&self) -> bool {
        false
    }

    async fn get_tile(&self, xyz: &Xyz, _url_query: &Option<UrlQuery>) -> Result<Tile, Error> {
        if let Some(tile) = self
            .mbtiles
            .get_tile(xyz.z, xyz.x, xyz.y)
            .await
            .map_err(|_| AquireConnError(self.id.clone()))?
        {
            Ok(tile)
        } else {
            debug!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            Ok(Vec::default())
        }
    }
}
