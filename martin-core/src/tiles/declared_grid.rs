//! A source served on a tile grid the config declares for it, since stored archives cannot say so themselves.

use async_trait::async_trait;
use martin_tile_utils::{TileCoord, TileData, TileGrid, TileInfo};
use tilejson::TileJSON;

use crate::CacheZoomRange;
#[cfg(feature = "postgres")]
use crate::tiles::postgres::ActiveQueryRegistry;
use crate::tiles::{BoxedSource, MartinCoreResult, Source, Tile, UrlQuery};

/// A source whose `z/x/y` addresses are declared to be on `grid`.
///
/// Every request passes straight through to the wrapped source.
/// The `TileJSON` gains the `tileGrid` key and the catalog entry names the grid.
#[derive(Clone, Debug)]
pub struct DeclaredGridSource {
    inner: BoxedSource,
    grid: TileGrid,
    tilejson: TileJSON,
}

impl DeclaredGridSource {
    /// Declares that `inner` serves its tiles on `grid`.
    #[must_use]
    pub fn new(inner: BoxedSource, grid: TileGrid) -> Self {
        let mut tilejson = inner.get_tilejson().clone();
        tilejson.other.insert(
            "tileGrid".to_owned(),
            serde_json::to_value(&grid)
                .expect("a validated tile grid holds only finite numbers and strings"),
        );
        Self {
            inner,
            grid,
            tilejson,
        }
    }
}

#[async_trait]
impl Source for DeclaredGridSource {
    fn get_id(&self) -> &str {
        self.inner.get_id()
    }

    fn get_tilejson(&self) -> &TileJSON {
        &self.tilejson
    }

    fn get_tile_info(&self) -> TileInfo {
        self.inner.get_tile_info()
    }

    fn tile_grid(&self) -> &TileGrid {
        &self.grid
    }

    fn clone_source(&self) -> BoxedSource {
        Box::new(self.clone())
    }

    fn get_version(&self) -> Option<String> {
        self.inner.get_version()
    }

    fn support_url_query(&self) -> bool {
        self.inner.support_url_query()
    }

    fn benefits_from_concurrent_scraping(&self) -> bool {
        self.inner.benefits_from_concurrent_scraping()
    }

    fn empty_tile_implies_empty_children(&self) -> bool {
        self.inner.empty_tile_implies_empty_children()
    }

    #[cfg(feature = "postgres")]
    fn cancel_registry(&self) -> Option<ActiveQueryRegistry> {
        self.inner.cancel_registry()
    }

    fn cache_zoom(&self) -> CacheZoomRange {
        self.inner.cache_zoom()
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        self.inner.get_tile(xyz, url_query).await
    }

    async fn get_tile_with_etag(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Tile> {
        self.inner.get_tile_with_etag(xyz, url_query).await
    }

    async fn try_reload(&self) -> MartinCoreResult<BoxedSource> {
        let inner = self.inner.try_reload().await?;
        Ok(Box::new(Self::new(inner, self.grid.clone())))
    }
}
