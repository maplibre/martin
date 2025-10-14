//! `PMTiles` tile source implementations.

use std::fmt::{Debug, Formatter};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use log::{trace, warn};
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
use object_store::ObjectStore;
use pmtiles::{
    AsyncPmTilesReader, Compression, DirCacheResult, Directory, DirectoryCache, ObjectStoreBackend,
    TileId, TileType,
};
use tilejson::TileJSON;

use crate::get_cached_value;
use crate::tiles::cache::{CacheKey, CacheValue, OptTileCache};
use crate::tiles::pmtiles::PmtilesError::{self, InvalidMetadata};
use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

/// [`pmtiles::Directory`] cache for `PMTiles` files.
#[derive(Clone, Debug)]
pub struct PmtCache {
    /// Unique identifier for this cache instance
    ///
    /// Uniqueness invariant is guaranteed by how the struct is constructed
    id: usize,
    /// Cache storing (id, offset) -> [`pmtiles::Directory`]
    ///
    /// Set to [`None`] to disable caching
    cache: OptTileCache,
}

impl From<OptTileCache> for PmtCache {
    fn from(cache: OptTileCache) -> Self {
        static NEXT_CACHE_ID: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(0));

        Self {
            id: NEXT_CACHE_ID.fetch_add(1, SeqCst),
            cache,
        }
    }
}

impl DirectoryCache for PmtCache {
    async fn get_dir_entry(&self, offset: usize, tile_id: TileId) -> DirCacheResult {
        if let Some(dir) = get_cached_value!(&self.cache, CacheValue::PmtDirectory, {
            CacheKey::PmtDirectory(self.id, offset)
        }) {
            dir.find_tile_id(tile_id).into()
        } else {
            DirCacheResult::NotCached
        }
    }

    async fn insert_dir(&self, offset: usize, directory: Directory) {
        if let Some(cache) = &self.cache {
            cache
                .insert(
                    CacheKey::PmtDirectory(self.id, offset),
                    CacheValue::PmtDirectory(directory),
                )
                .await;
        }
    }
}

/// A source for `PMTiles` files using `ObjectStoreBackend`
#[derive(Clone)]
pub struct PmtilesSource {
    id: String,
    pmtiles: Arc<AsyncPmTilesReader<ObjectStoreBackend, PmtCache>>,
    tilejson: TileJSON,
    tile_info: TileInfo,
}

#[expect(clippy::missing_fields_in_debug)]
impl Debug for PmtilesSource {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PmtilesSource")
            .field("id", &self.id)
            .finish()
    }
}

impl PmtilesSource {
    /// Create a new `PmtilesSource` from an id, an [`ObjectStore`] and the path of the `.pmtiles` file
    pub async fn new(
        cache: PmtCache,
        id: String,
        store: Box<dyn ObjectStore>,
        path: impl Into<object_store::path::Path>,
    ) -> Result<Self, PmtilesError> {
        let path = path.into();
        let store_to_string = store.to_string();
        let backend = ObjectStoreBackend::new(store, path.clone());
        let reader = AsyncPmTilesReader::try_from_cached_source(backend, cache)
            .await
            .map_err(|e| PmtilesError::PmtErrorWithCtx(e, store_to_string.clone()))?;

        let hdr = &reader.get_header();

        if hdr.tile_type != TileType::Mvt && hdr.tile_compression != Compression::None {
            return Err(InvalidMetadata(
                format!(
                    "Format {:?} and compression {:?} are not yet supported",
                    hdr.tile_type, hdr.tile_compression
                ),
                path.clone(),
            ));
        }

        let format = match hdr.tile_type {
            TileType::Mvt => TileInfo::new(
                Format::Mvt,
                match hdr.tile_compression {
                    Compression::None => Encoding::Uncompressed,
                    Compression::Unknown => {
                        warn!(
                            "MVT tiles of source {id} ({store_to_string} at {path}) has unknown compression"
                        );
                        Encoding::Uncompressed
                    }
                    Compression::Gzip => Encoding::Gzip,
                    Compression::Brotli => Encoding::Brotli,
                    Compression::Zstd => Encoding::Zstd,
                },
            ),
            // All these assume uncompressed data (validated above)
            TileType::Png => Format::Png.into(),
            TileType::Jpeg => Format::Jpeg.into(),
            TileType::Webp => Format::Webp.into(),
            TileType::Unknown => {
                return Err(PmtilesError::UnknownTileType(
                    id.clone(),
                    store_to_string.clone(),
                    path.to_string(),
                ));
            }
        };

        let tilejson = reader.parse_tilejson(Vec::new()).await.unwrap_or_else(|e| {
            warn!("{e:?}: Unable to parse metadata for {path}");
            hdr.get_tilejson(Vec::new())
        });

        Ok(Self {
            id,
            pmtiles: Arc::new(reader),
            tilejson,
            tile_info: format,
        })
    }
}
#[async_trait]
impl Source for PmtilesSource {
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
        true
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        let coord = pmtiles::TileCoord::new(xyz.z, xyz.x, xyz.y).map_err(PmtilesError::PmtError)?;
        if let Some(t) = self
            .pmtiles
            .get_tile(coord)
            .await
            .map_err(PmtilesError::PmtError)?
        {
            Ok(t.to_vec())
        } else {
            trace!(
                "Couldn't find tile data in {}/{}/{} of {}",
                xyz.z, xyz.x, xyz.y, &self.id
            );
            Ok(Vec::new())
        }
    }
}
