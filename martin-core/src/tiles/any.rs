//! [`AnySource`], the closed set of tile sources the server dispatches on.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use martin_tile_utils::{TileCoord, TileData, TileGrid, TileInfo};
use tilejson::TileJSON;

use crate::CacheZoomRange;
use crate::tiles::catalog::CatalogSourceEntry;
#[cfg(feature = "unstable-cog")]
use crate::tiles::cog::CogSource;
use crate::tiles::declared_grid::DeclaredGridSource;
#[cfg(feature = "unstable-duckdb")]
use crate::tiles::duckdb::DuckDBSource;
#[cfg(feature = "geojson")]
use crate::tiles::geojson::source::GeoJsonSource;
#[cfg(feature = "mbtiles")]
use crate::tiles::mbtiles::MbtSource;
#[cfg(feature = "passthrough")]
use crate::tiles::passthrough::PassthroughSource;
#[cfg(feature = "pmtiles")]
use crate::tiles::pmtiles::PmtilesSource;
#[cfg(feature = "postgres")]
use crate::tiles::postgres::{ActiveQueryRegistry, PostgresSource, PostgresTileFeatures};
#[cfg(feature = "_testing")]
use crate::tiles::testing::TestSource;
use crate::tiles::{BoxedSource, MartinCoreResult, Source as _, Tile, UrlQuery};

/// Every tile source the server can serve.
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "always behind an Arc, and a declared grid always wraps a backend"
)]
pub enum AnySource {
    /// A backend served on the grid it reports itself.
    Backend(BackendSource),
    /// A backend served on a tile grid the config declares for it.
    DeclaredGrid(DeclaredGridSource),
}

/// A source that reads tiles from a storage or compute backend.
#[derive(Debug)]
pub enum BackendSource {
    /// A `.pmtiles` archive.
    #[cfg(feature = "pmtiles")]
    Pmtiles(PmtilesSource),
    /// A `.mbtiles` archive.
    #[cfg(feature = "mbtiles")]
    Mbtiles(MbtSource),
    /// A `PostGIS` table or function.
    #[cfg(feature = "postgres")]
    Postgres(PostgresSource),
    /// An upstream HTTP tile server.
    #[cfg(feature = "passthrough")]
    Passthrough(PassthroughSource),
    /// A `.geojson` file tiled on the fly.
    #[cfg(feature = "geojson")]
    GeoJson(GeoJsonSource),
    /// A cloud-optimized `GeoTIFF`.
    #[cfg(feature = "unstable-cog")]
    Cog(CogSource),
    /// A `DuckDB` query.
    #[cfg(feature = "unstable-duckdb")]
    DuckDb(DuckDBSource),
    /// A configurable double, for tests and benchmarks.
    #[cfg(feature = "_testing")]
    Test(TestSource),
}

/// Runs `$body` against whichever backend `$self` holds.
macro_rules! dispatch_backend {
    ($self:expr, |$s:ident| $body:expr) => {
        match $self {
            #[cfg(feature = "pmtiles")]
            BackendSource::Pmtiles($s) => $body,
            #[cfg(feature = "mbtiles")]
            BackendSource::Mbtiles($s) => $body,
            #[cfg(feature = "postgres")]
            BackendSource::Postgres($s) => $body,
            #[cfg(feature = "passthrough")]
            BackendSource::Passthrough($s) => $body,
            #[cfg(feature = "geojson")]
            BackendSource::GeoJson($s) => $body,
            #[cfg(feature = "unstable-cog")]
            BackendSource::Cog($s) => $body,
            #[cfg(feature = "unstable-duckdb")]
            BackendSource::DuckDb($s) => $body,
            #[cfg(feature = "_testing")]
            BackendSource::Test($s) => $body,
        }
    };
}

/// Runs `$body` against whichever source `$self` holds, backend or declared grid.
macro_rules! dispatch {
    ($self:expr, |$s:ident| $body:expr) => {
        match $self {
            AnySource::Backend(b) => dispatch_backend!(b, |$s| $body),
            AnySource::DeclaredGrid($s) => $body,
        }
    };
}

macro_rules! impl_from_backend {
    ($($(#[$cfg:meta])* $variant:ident($ty:ty)),* $(,)?) => {
        $(
            $(#[$cfg])*
            impl From<$ty> for BackendSource {
                fn from(source: $ty) -> Self {
                    Self::$variant(source)
                }
            }
        )*
    };
}

impl_from_backend! {
    #[cfg(feature = "pmtiles")] Pmtiles(PmtilesSource),
    #[cfg(feature = "mbtiles")] Mbtiles(MbtSource),
    #[cfg(feature = "postgres")] Postgres(PostgresSource),
    #[cfg(feature = "passthrough")] Passthrough(PassthroughSource),
    #[cfg(feature = "geojson")] GeoJson(GeoJsonSource),
    #[cfg(feature = "unstable-cog")] Cog(CogSource),
    #[cfg(feature = "unstable-duckdb")] DuckDb(DuckDBSource),
    #[cfg(feature = "_testing")] Test(TestSource),
}

impl BackendSource {
    /// This source as a shared handle, ready for a registry.
    #[must_use]
    pub fn boxed(self) -> BoxedSource {
        Arc::new(AnySource::Backend(self))
    }

    /// This source as a shared handle, declared to be on `grid` when there is one.
    #[must_use]
    pub fn boxed_on(self, grid: Option<&TileGrid>) -> BoxedSource {
        match grid {
            Some(grid) => DeclaredGridSource::new(self, grid.clone()).boxed(),
            None => self.boxed(),
        }
    }

    /// Unique source identifier used in URLs.
    #[must_use]
    pub fn get_id(&self) -> &str {
        dispatch_backend!(self, |s| s.get_id())
    }

    /// `TileJSON` specification served to clients.
    #[must_use]
    pub fn get_tilejson(&self) -> &TileJSON {
        dispatch_backend!(self, |s| s.get_tilejson())
    }

    /// Technical tile information (format, encoding, etc.).
    #[must_use]
    pub fn get_tile_info(&self) -> TileInfo {
        dispatch_backend!(self, |s| s.get_tile_info())
    }

    /// A version string for this source, if available.
    #[must_use]
    pub fn get_version(&self) -> Option<String> {
        dispatch_backend!(self, |s| s.get_version())
    }

    /// Whether this source accepts URL query parameters.
    #[must_use]
    pub fn support_url_query(&self) -> bool {
        dispatch_backend!(self, |s| s.support_url_query())
    }

    /// Whether `martin cp` should use concurrent scraping.
    #[must_use]
    pub fn benefits_from_concurrent_scraping(&self) -> bool {
        dispatch_backend!(self, |s| s.benefits_from_concurrent_scraping())
    }

    /// Whether an empty tile implies that all tiles below it are empty.
    #[must_use]
    pub fn empty_tile_implies_empty_children(&self) -> bool {
        dispatch_backend!(self, |s| s.empty_tile_implies_empty_children())
    }

    /// Zoom-level bounds for tile caching.
    #[must_use]
    pub fn cache_zoom(&self) -> CacheZoomRange {
        dispatch_backend!(self, |s| s.cache_zoom())
    }

    /// The `PostgreSQL` source, if this is one.
    #[cfg(feature = "postgres")]
    #[must_use]
    #[cfg_attr(
        not(any(
            feature = "pmtiles",
            feature = "mbtiles",
            feature = "passthrough",
            feature = "geojson",
            feature = "unstable-cog",
            feature = "unstable-duckdb",
            feature = "_testing",
        )),
        expect(
            irrefutable_let_patterns,
            reason = "Postgres is the only variant in a postgres-only build"
        )
    )]
    pub fn as_postgres(&self) -> Option<&PostgresSource> {
        if let Self::Postgres(s) = self {
            Some(s)
        } else {
            None
        }
    }

    /// Retrieves tile data for the given coordinates.
    pub async fn get_tile(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        let tile: Pin<Box<dyn Future<Output = MartinCoreResult<TileData>> + Send + '_>> =
            dispatch_backend!(self, |s| Box::pin(s.get_tile(xyz, url_query)));
        tile.await
    }

    /// Retrieves tile with etag for the given coordinates.
    pub async fn get_tile_with_etag(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Tile> {
        let tile: Pin<Box<dyn Future<Output = MartinCoreResult<Tile>> + Send + '_>> =
            dispatch_backend!(self, |s| Box::pin(s.get_tile_with_etag(xyz, url_query)));
        tile.await
    }

    /// Attempts to create a fresh instance of this source.
    pub async fn try_reload(&self) -> MartinCoreResult<Self> {
        dispatch_backend!(self, |s| s.try_reload().await.map(Self::from))
    }
}

impl AnySource {
    /// The backend this source reads its tiles from.
    #[must_use]
    pub fn backend(&self) -> &BackendSource {
        match self {
            Self::Backend(b) => b,
            Self::DeclaredGrid(s) => s.inner(),
        }
    }

    /// Unique source identifier used in URLs.
    #[must_use]
    pub fn get_id(&self) -> &str {
        dispatch!(self, |s| s.get_id())
    }

    /// `TileJSON` specification served to clients.
    #[must_use]
    pub fn get_tilejson(&self) -> &TileJSON {
        dispatch!(self, |s| s.get_tilejson())
    }

    /// Technical tile information (format, encoding, etc.).
    #[must_use]
    pub fn get_tile_info(&self) -> TileInfo {
        dispatch!(self, |s| s.get_tile_info())
    }

    /// The tile grid this source's `z/x/y` addresses refer to.
    #[must_use]
    pub fn tile_grid(&self) -> &TileGrid {
        dispatch!(self, |s| s.tile_grid())
    }

    /// A version string for this source, if available.
    #[must_use]
    pub fn get_version(&self) -> Option<String> {
        self.backend().get_version()
    }

    /// Whether this source accepts URL query parameters.
    #[must_use]
    pub fn support_url_query(&self) -> bool {
        self.backend().support_url_query()
    }

    /// Whether `martin cp` should use concurrent scraping.
    #[must_use]
    pub fn benefits_from_concurrent_scraping(&self) -> bool {
        self.backend().benefits_from_concurrent_scraping()
    }

    /// Whether an empty tile implies that all tiles below it are empty.
    #[must_use]
    pub fn empty_tile_implies_empty_children(&self) -> bool {
        self.backend().empty_tile_implies_empty_children()
    }

    /// The cancellation registry for in-flight queries, where the backend has one.
    #[cfg(feature = "postgres")]
    #[must_use]
    pub fn cancel_registry(&self) -> Option<ActiveQueryRegistry> {
        self.backend()
            .as_postgres()
            .map(PostgresSource::active_query_registry)
    }

    /// Zoom-level bounds for tile caching.
    #[must_use]
    pub fn cache_zoom(&self) -> CacheZoomRange {
        self.backend().cache_zoom()
    }

    /// Validates zoom level against `TileJSON` min/max zoom constraints.
    #[must_use]
    pub fn is_valid_zoom(&self, zoom: u8) -> bool {
        dispatch!(self, |s| s.is_valid_zoom(zoom))
    }

    /// Generates catalog entry for this source.
    #[must_use]
    pub fn get_catalog_entry(&self) -> CatalogSourceEntry {
        dispatch!(self, |s| s.get_catalog_entry())
    }

    /// Retrieves tile data for the given coordinates.
    pub async fn get_tile(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData> {
        self.backend().get_tile(xyz, url_query).await
    }

    /// Retrieves the features of a tile instead of its serialized bytes.
    ///
    /// `None` means this source cannot hand out features, and the caller has to fall back to
    /// [`get_tile`](Self::get_tile).
    #[cfg(feature = "postgres")]
    pub async fn get_tile_features(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
        keep_measures: bool,
    ) -> MartinCoreResult<Option<PostgresTileFeatures>> {
        match self.backend().as_postgres() {
            Some(source) => {
                source
                    .get_tile_features(xyz, url_query, keep_measures)
                    .await
            }
            None => Ok(None),
        }
    }

    /// Retrieves tile with etag for the given coordinates.
    pub async fn get_tile_with_etag(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Tile> {
        self.backend().get_tile_with_etag(xyz, url_query).await
    }

    /// Attempts to create a fresh instance of this source.
    pub async fn try_reload(&self) -> MartinCoreResult<BoxedSource> {
        let reloaded = match self {
            Self::Backend(b) => Self::Backend(b.try_reload().await?),
            Self::DeclaredGrid(s) => Self::DeclaredGrid(s.try_reload().await?),
        };
        Ok(Arc::new(reloaded))
    }
}
