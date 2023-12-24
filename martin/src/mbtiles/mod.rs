use std::fmt::{Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use log::trace;
use martin_tile_utils::TileInfo;
use mbtiles::MbtilesPool;
use serde::{Deserialize, Serialize};
use tilejson::TileJSON;

use crate::file_config::FileError::{AquireConnError, InvalidMetadata, IoError};
use crate::file_config::{FileConfigExtras, FileResult};
use crate::source::{TileData, UrlQuery};
use crate::{MartinResult, Source, TileCoord};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MbtilesConfig;

#[async_trait]
impl FileConfigExtras for MbtilesConfig {
    async fn new_sources(&self, id: String, path: PathBuf) -> MartinResult<Box<dyn Source>> {
        Ok(Box::new(MbtSource::new(id, path).await?))
    }
}

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
    async fn new(id: String, path: PathBuf) -> FileResult<Self> {
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
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.tile_info
    }

    fn clone_source(&self) -> Box<dyn Source> {
        Box::new(self.clone())
    }

    async fn get_tile(
        &self,
        xyz: &TileCoord,
        _url_query: &Option<UrlQuery>,
    ) -> MartinResult<TileData> {
        if let Some(tile) = self
            .mbtiles
            .get_tile(xyz.z, xyz.x, xyz.y)
            .await
            .map_err(|_| AquireConnError(self.id.clone()))?
        {
            Ok(tile)
        } else {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z,
                xyz.x,
                xyz.y,
                &self.id
            );
            Ok(Vec::new())
        }
    }
}
