//! `MBTiles` tile source implementation.

use std::fmt::{Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use log::trace;
use martin_tile_utils::{TileCoord, TileData, TileInfo};
use mbtiles::MbtilesPool;
use tilejson::TileJSON;

use crate::tiles::mbtiles::MbtilesError;
use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

/// Tile source that reads from `MBTiles` files.
#[derive(Clone)]
pub struct MbtSource {
    id: String,
    mbtiles: Arc<MbtilesPool>,
    tilejson: TileJSON,
    tile_info: TileInfo,
}

#[allow(clippy::missing_fields_in_debug)]
impl Debug for MbtSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MbtSource")
            .field("id", &self.id)
            .field("path", &self.mbtiles.as_ref())
            .finish()
    }
}

impl MbtSource {
    /// Creates a new `MBTiles` source from the given file path.
    pub async fn new(id: String, path: PathBuf) -> Result<Self, MbtilesError> {
        let mbt = MbtilesPool::open_readonly(&path)
            .await
            .map_err(|e| io::Error::other(format!("{e:?}: Cannot open file {}", path.display())))
            .map_err(|e| MbtilesError::IoError(e, path.clone()))?;

        let meta = mbt
            .get_metadata()
            .await
            .map_err(|e| MbtilesError::InvalidMetadata(e.to_string(), path))?;

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

    fn clone_source(&self) -> BoxedSource {
        Box::new(self.clone())
    }

    fn benefits_from_concurrent_scraping(&self) -> bool {
        // If we copy from one local file to another, we are likely not bottlenecked by CPU
        false
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        if let Some(tile) = self
            .mbtiles
            .get_tile(xyz.z, xyz.x, xyz.y)
            .await
            .map_err(|_| MbtilesError::AcquireConnError(self.id.clone()))?
        {
            Ok(tile)
        } else {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            Ok(Vec::new())
        }
    }
}
