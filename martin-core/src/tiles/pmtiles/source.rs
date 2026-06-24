//! `PMTiles` tile source implementations.

use std::sync::Arc;

use async_trait::async_trait;
use derive_debug::Dbg;
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
use object_store::ObjectStore;
use pmtiles::{AsyncPmTilesReader, Compression, ObjectStoreBackend, PmtError, TileType};
use tilejson::TileJSON;
use tracing::{trace, warn};

use crate::CacheZoomRange;
use crate::tiles::pmtiles::PmtCacheInstance;
use crate::tiles::pmtiles::PmtilesError::{self, InvalidMetadata};
use crate::tiles::{BoxedSource, MartinCoreError, MartinCoreResult, Source, UrlQuery};

/// A source for `PMTiles` files using `ObjectStoreBackend`
#[derive(Clone, Dbg)]
pub struct PmtilesSource {
    id: String,
    #[dbg(skip)]
    pmtiles: Arc<AsyncPmTilesReader<ObjectStoreBackend, PmtCacheInstance>>,
    #[dbg(skip)]
    tilejson: TileJSON,
    #[dbg(skip)]
    tile_info: TileInfo,
    #[dbg(skip)]
    cache_zoom: CacheZoomRange,

    #[dbg(skip)]
    store: Arc<dyn ObjectStore>,
    #[dbg(skip)]
    path: object_store::path::Path,
    #[dbg(skip)]
    pmt_cache: PmtCacheInstance,
}

impl PmtilesSource {
    /// Create a new `PmtilesSource` from an id, an [`ObjectStore`] and the path of the `.pmtiles` file
    pub async fn new(
        cache: PmtCacheInstance,
        id: String,
        store: Box<dyn ObjectStore>,
        path: impl Into<object_store::path::Path>,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, PmtilesError> {
        let path = path.into();
        // Wrap in Arc so we can clone the store cheaply for try_reload.
        let store: Arc<dyn ObjectStore> = Arc::from(store);
        let store_to_string = store.to_string();
        let backend = ObjectStoreBackend::new(Box::new(Arc::clone(&store)), path.clone());
        let reader = AsyncPmTilesReader::try_from_cached_source(backend, cache.clone())
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
                            source.id = %id,
                            store = %store_to_string,
                            path = %path,
                            "MVT tiles have unknown compression"
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
            TileType::Avif => Format::Avif.into(),
            TileType::Mlt => Format::Mlt.into(),
            TileType::Unknown => {
                return Err(PmtilesError::UnknownTileType {
                    source_id: id.clone(),
                    store: store_to_string.clone(),
                    path: path.to_string(),
                });
            }
        };

        let tilejson = reader.parse_tilejson(Vec::new()).await.unwrap_or_else(|e| {
            warn!(path = %path, error = ?e, "Unable to parse metadata");
            hdr.get_tilejson(Vec::new())
        });

        Ok(Self {
            id,
            pmtiles: Arc::new(reader),
            tilejson,
            tile_info: format,
            cache_zoom,
            store,
            path,
            pmt_cache: cache,
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

    async fn try_reload(&self) -> MartinCoreResult<BoxedSource> {
        Self::new(
            self.pmt_cache.fork(),
            self.id.clone(),
            Box::new(Arc::clone(&self.store)),
            self.path.clone(),
            self.cache_zoom,
        )
        .await
        .map(|s| Box::new(s) as BoxedSource)
        .map_err(MartinCoreError::from)
    }

    fn cache_zoom(&self) -> CacheZoomRange {
        self.cache_zoom
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        let coord = pmtiles::TileCoord::new(xyz.z, xyz.x, xyz.y).map_err(PmtilesError::PmtError)?;
        let result = self.pmtiles.get_tile(coord).await;
        if let Some(t) = match result {
            Err(PmtError::SourceModified) => {
                return Err(MartinCoreError::SourceNeedsReload);
            }
            Err(e) => return Err(PmtilesError::PmtError(e).into()),
            Ok(t) => t,
        } {
            Ok(t.to_vec())
        } else {
            trace!(
                source.id = %self.id,
                tile.z = xyz.z,
                tile.x = xyz.x,
                tile.y = xyz.y,
                "Couldn't find tile data"
            );
            Ok(Vec::new())
        }
    }
}
