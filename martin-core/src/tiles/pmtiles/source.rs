//! `PMTiles` tile source implementations.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use derive_debug::Dbg;
use futures::Future;
use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
use object_store::ObjectStore;
use pmtiles::{AsyncPmTilesReader, Compression, ObjectStoreBackend, TileType};
use tilejson::TileJSON;
use tokio::sync::RwLock;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{trace, warn};
use url::Url;

use crate::CacheZoomRange;
use crate::tiles::pmtiles::PmtilesError::{self, InvalidMetadata};
use crate::tiles::pmtiles::{PmtCache, PmtCacheInstance};
use crate::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};

/// Process-wide monotonically-increasing cache id minted on every `PMTiles` source (re)build.
/// The shared [`PmtCache`] is keyed on `(cache_id, offset)`, so unique ids guarantee that
/// directory entries from a prior reader are unreachable from a freshly-built one.
fn next_pmtiles_cache_id() -> usize {
    static NEXT_CACHE_ID: LazyLock<AtomicUsize> = LazyLock::new(|| AtomicUsize::new(0));
    NEXT_CACHE_ID.fetch_add(1, Ordering::SeqCst)
}

/// Async closure that yields a fresh `AsyncPmTilesReader` each time it is invoked, used by
/// [`PmtilesSource`] to rebuild itself in place when the underlying blob's
/// `data_version_string` (`ETag`) changes.
pub type ReaderRebuilder = Arc<
    dyn Fn() -> Pin<
            Box<
                dyn Future<
                        Output = Result<
                            AsyncPmTilesReader<ObjectStoreBackend, PmtCacheInstance>,
                            PmtilesError,
                        >,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

/// A source for `PMTiles` files using `ObjectStoreBackend`.
#[derive(Clone, Dbg)]
pub struct PmtilesSource {
    id: String,
    /// Hot-swappable reader. Wrapping in `Arc<RwLock<Arc<…>>>` lets clones share the same
    /// rebuildable instance: a successful in-source rebuild on one clone is visible to all.
    #[dbg(skip)]
    reader: Arc<RwLock<Arc<AsyncPmTilesReader<ObjectStoreBackend, PmtCacheInstance>>>>,
    /// Set at construction; not refreshed on rebuild. Tile metadata changes are uncommon for
    /// a logical tileset, so the trade-off favours a sync `get_tilejson(&self) -> &TileJSON`.
    #[dbg(skip)]
    tilejson: TileJSON,
    #[dbg(skip)]
    tile_info: TileInfo,
    #[dbg(skip)]
    cache_zoom: CacheZoomRange,
    /// Optional rebuilder closure. If set, `get_tile` will rebuild the reader on
    /// `PmtError::SourceModified` and retry the fetch once before returning to the caller.
    #[dbg(skip)]
    rebuilder: Option<ReaderRebuilder>,
    /// Optional channel kicked after a successful in-source rebuild so a reloader can
    /// invalidate any stale tile-cache entries for this source. The receiver may be gone if
    /// the reloader is shutting down — sends are best-effort.
    #[dbg(skip)]
    reload_signal: Option<UnboundedSender<String>>,
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
            TileType::Avif => Format::Avif.into(),
            TileType::Mlt => Format::Mlt.into(),
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
            reader: Arc::new(RwLock::new(Arc::new(reader))),
            tilejson,
            tile_info: format,
            cache_zoom,
            rebuilder: None,
            reload_signal: None,
        })
    }

    /// Attach a rebuilder closure. With one configured, `get_tile` transparently rebuilds the
    /// reader and retries on `PmtError::SourceModified` (the underlying blob's
    /// `data_version_string` changed since this reader was constructed), so callers see a
    /// successful response on the very same request that triggered detection.
    #[must_use]
    pub fn with_rebuilder(mut self, rebuilder: ReaderRebuilder) -> Self {
        self.rebuilder = Some(rebuilder);
        self
    }

    /// Build a `PmtilesSource` from an `object_store` URL plus options, with self-rebuild
    /// already wired up. Each rebuild parses the URL afresh and mints a fresh
    /// [`PmtCacheInstance`] so directory entries from the prior reader are unreachable from
    /// the new one and age out via the moka TTL.
    pub async fn from_object_store_url(
        id: String,
        url: Url,
        options: HashMap<String, String>,
        pmt_cache: PmtCache,
        cache_zoom: CacheZoomRange,
    ) -> Result<Self, PmtilesError> {
        let dir_cache = PmtCacheInstance::new(next_pmtiles_cache_id(), pmt_cache.clone());
        let (store, path) = object_store::parse_url_opts(&url, &options)
            .map_err(|e| PmtilesError::ObjectStoreParse(e, url.to_string()))?;
        let source = Self::new(dir_cache, id.clone(), store, path, cache_zoom).await?;

        let rebuilder: ReaderRebuilder = Arc::new(move || {
            let url = url.clone();
            let options = options.clone();
            let pmt_cache = pmt_cache.clone();
            let id = id.clone();
            Box::pin(async move {
                let (store, path) = object_store::parse_url_opts(&url, &options)
                    .map_err(|e| PmtilesError::ObjectStoreParse(e, url.to_string()))?;
                let dir_cache = PmtCacheInstance::new(next_pmtiles_cache_id(), pmt_cache);
                let store_to_string = store.to_string();
                let backend = ObjectStoreBackend::new(store, path);
                AsyncPmTilesReader::try_from_cached_source(backend, dir_cache)
                    .await
                    .map_err(|e| {
                        PmtilesError::PmtErrorWithCtx(e, format!("{id} ({store_to_string})"))
                    })
            })
        });
        Ok(source.with_rebuilder(rebuilder))
    }

    /// Attach a reload-signal channel. Kicked (best-effort) on every in-source rebuild so a
    /// reloader can invalidate stale tile-cache entries for this source's id.
    #[must_use]
    pub fn with_reload_signal(mut self, signal: UnboundedSender<String>) -> Self {
        self.reload_signal = Some(signal);
        self
    }

    /// Acquire a write lock and replace the inner reader with a fresh one. Uses
    /// double-checked equality on the held `Arc` so concurrent `SourceModified` detections
    /// don't all rebuild redundantly.
    async fn rebuild_if_stale(
        &self,
        previous: &Arc<AsyncPmTilesReader<ObjectStoreBackend, PmtCacheInstance>>,
    ) -> Result<(), PmtilesError> {
        let Some(rebuilder) = &self.rebuilder else {
            return Err(PmtilesError::PmtError(pmtiles::PmtError::SourceModified));
        };
        let mut guard = self.reader.write().await;
        if !Arc::ptr_eq(&*guard, previous) {
            // Another concurrent caller already rebuilt; nothing to do.
            return Ok(());
        }
        let fresh = (rebuilder)().await?;
        *guard = Arc::new(fresh);
        Ok(())
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

    fn cache_zoom(&self) -> CacheZoomRange {
        self.cache_zoom
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        _url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        let coord = pmtiles::TileCoord::new(xyz.z, xyz.x, xyz.y).map_err(PmtilesError::PmtError)?;

        // Snapshot the current reader Arc out of the lock so the actual fetch happens unlocked.
        let reader = {
            let guard = self.reader.read().await;
            guard.clone()
        };

        match reader.get_tile(coord).await {
            Ok(Some(t)) => Ok(t.to_vec()),
            Ok(None) => {
                trace!(
                    "Couldn't find tile data in {}/{}/{} of {}",
                    xyz.z, xyz.x, xyz.y, &self.id
                );
                Ok(Vec::new())
            }
            Err(pmtiles::PmtError::SourceModified) => {
                trace!(
                    "PMTiles source {} reports SourceModified; rebuilding in place",
                    self.id
                );
                self.rebuild_if_stale(&reader).await?;
                // Best-effort: notify any registered reloader so it can invalidate stale
                // tile-cache entries for this source id.
                if let Some(tx) = &self.reload_signal {
                    let _ = tx.send(self.id.clone());
                }
                let fresh = {
                    let guard = self.reader.read().await;
                    guard.clone()
                };
                match fresh.get_tile(coord).await {
                    Ok(Some(t)) => Ok(t.to_vec()),
                    Ok(None) => Ok(Vec::new()),
                    Err(e) => Err(PmtilesError::PmtError(e).into()),
                }
            }
            Err(e) => Err(PmtilesError::PmtError(e).into()),
        }
    }
}
