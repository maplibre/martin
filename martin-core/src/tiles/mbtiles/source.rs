//! `MBTiles` tile source implementation.

use std::fmt::{Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use martin_tile_utils::{TileCoord, TileData, TileInfo};
use mbtiles::{MbtError, MbtType, MbtilesPool};
use tilejson::TileJSON;
use tracing::trace;

use crate::tiles::mbtiles::MbtilesError;
use crate::tiles::{BoxedSource, MartinCoreResult, Source, Tile, UrlQuery};

/// Tile source that reads from `MBTiles` files.
#[derive(Clone)]
pub struct MbtSource {
    id: String,
    mbtiles: Arc<MbtilesPool>,
    tilejson: TileJSON,
    tile_info: TileInfo,
    mbt_type: Option<MbtType>,
}

#[expect(clippy::missing_fields_in_debug)]
impl Debug for MbtSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MbtSource")
            .field("id", &self.id)
            .field("path", &self.mbtiles.as_ref())
            .field("mbt_type", &self.mbt_type.as_ref().map(|t| format!("{t:?}")).unwrap_or_else(|| "Unknown".to_string()))
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
            .map_err(|e| MbtilesError::InvalidMetadata(e.to_string(), path.clone()))?;

        // Empty mbtiles should cause an error
        let tile_info = mbt
            .detect_format(&meta.tilejson)
            .await
            .and_then(|v| v.ok_or(MbtError::NoTilesFound))
            .map_err(|e| MbtilesError::InvalidMetadata(e.to_string(), path.clone()))?;

        // Try to detect the MBTiles schema type, but don't fail if we can't
        // This allows working with test databases that don't have proper uniqueness constraints
        let mbt_type = mbt.detect_type().await.ok();

        Ok(Self {
            id,
            mbtiles: Arc::new(mbt),
            tilejson: meta.tilejson,
            tile_info,
            mbt_type,
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

    fn get_version(&self) -> Option<String> {
        self.tilejson.version.clone()
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

    async fn get_tile_with_etag(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Tile> {
        // If we have a known type, use it to get tile and hash efficiently
        if let Some(mbt_type) = self.mbt_type {
            if let Some((data, hash)) = self
                .mbtiles
                .get_tile_and_hash(mbt_type, xyz.z, xyz.x, xyz.y)
                .await
                .map_err(MbtilesError::MbtilesLibraryError)?
            {
                if let Some(hash_str) = hash {
                    return Ok(Tile::new_with_etag(data, self.tile_info, hash_str));
                } else {
                    return Ok(Tile::new_hash_etag(data, self.tile_info));
                }
            } else {
                // Tile not found - return empty tile with computed etag
                // This matches the behavior of get_tile() for consistency
                trace!(
                    "Couldn't find tile data in {}/{}/{} of {}",
                    xyz.z, xyz.x, xyz.y, &self.id
                );
                return Ok(Tile::new_hash_etag(Vec::new(), self.tile_info));
            }
        }

        // Fallback: try each type in order of likelihood
        // First try FlatWithHash (has hash column)
        if let Ok(Some((data, Some(hash_str)))) = self
            .mbtiles
            .get_tile_and_hash(MbtType::FlatWithHash, xyz.z, xyz.x, xyz.y)
            .await
        {
            return Ok(Tile::new_with_etag(data, self.tile_info, hash_str));
        }

        // Then try Normalized with hash view
        if let Ok(Some((data, Some(hash_str)))) = self
            .mbtiles
            .get_tile_and_hash(MbtType::Normalized { hash_view: true }, xyz.z, xyz.x, xyz.y)
            .await
        {
            return Ok(Tile::new_with_etag(data, self.tile_info, hash_str));
        }

        // Then try Normalized without hash view
        if let Ok(Some((data, Some(hash_str)))) = self
            .mbtiles
            .get_tile_and_hash(MbtType::Normalized { hash_view: false }, xyz.z, xyz.x, xyz.y)
            .await
        {
            return Ok(Tile::new_with_etag(data, self.tile_info, hash_str));
        }

        // Finally fallback to Flat (no hash) - compute etag
        if let Some(tile) = self
            .mbtiles
            .get_tile(xyz.z, xyz.x, xyz.y)
            .await
            .map_err(|_| MbtilesError::AcquireConnError(self.id.clone()))?
        {
            Ok(Tile::new_hash_etag(tile, self.tile_info))
        } else {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            Ok(Tile::new_hash_etag(Vec::new(), self.tile_info))
        }
    }
}
