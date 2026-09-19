//! [`AnySource`], the closed set of tile sources the server dispatches on.
//!
//! Built-in sources are reached by a `match` rather than a vtable, so their
//! `get_tile_with_etag` is a direct call instead of an `#[async_trait]` boxed
//! future.

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
use crate::tiles::postgres::{ActiveQueryRegistry, PostgresSource};
#[cfg(feature = "_testing")]
use crate::tiles::testing::TestSource;
use crate::tiles::{MartinCoreResult, Source as _, Tile, UrlQuery};

/// Every tile source the server can serve.
#[derive(Debug)]
pub enum AnySource {
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
    /// Any of the above, served on a tile grid the config declares for it.
    DeclaredGrid(DeclaredGridSource),
}

/// Runs `$body` against whichever source `$self` holds.
macro_rules! dispatch {
    ($self:expr, |$s:ident| $body:expr) => {
        match $self {
            #[cfg(feature = "pmtiles")]
            AnySource::Pmtiles($s) => $body,
            #[cfg(feature = "mbtiles")]
            AnySource::Mbtiles($s) => $body,
            #[cfg(feature = "postgres")]
            AnySource::Postgres($s) => $body,
            #[cfg(feature = "passthrough")]
            AnySource::Passthrough($s) => $body,
            #[cfg(feature = "geojson")]
            AnySource::GeoJson($s) => $body,
            #[cfg(feature = "unstable-cog")]
            AnySource::Cog($s) => $body,
            #[cfg(feature = "unstable-duckdb")]
            AnySource::DuckDb($s) => $body,
            #[cfg(feature = "_testing")]
            AnySource::Test($s) => $body,
            AnySource::DeclaredGrid($s) => $body,
        }
    };
}

impl AnySource {
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
        dispatch!(self, |s| s.get_version())
    }

    /// Whether this source accepts URL query parameters.
    #[must_use]
    pub fn support_url_query(&self) -> bool {
        dispatch!(self, |s| s.support_url_query())
    }

    /// Whether `martin cp` should use concurrent scraping.
    #[must_use]
    pub fn benefits_from_concurrent_scraping(&self) -> bool {
        dispatch!(self, |s| s.benefits_from_concurrent_scraping())
    }

    /// Whether an empty tile implies that all tiles below it are empty.
    #[must_use]
    pub fn empty_tile_implies_empty_children(&self) -> bool {
        dispatch!(self, |s| s.empty_tile_implies_empty_children())
    }

    /// The cancellation registry for in-flight queries, where the backend has one.
    ///
    /// Only Postgres has one, and with a closed set that is a match arm rather
    /// than a `#[cfg]`-gated method every other source has to carry.
    #[cfg(feature = "postgres")]
    #[must_use]
    pub fn cancel_registry(&self) -> Option<ActiveQueryRegistry> {
        match self {
            Self::Postgres(s) => Some(s.active_query_registry()),
            #[cfg(feature = "pmtiles")]
            Self::Pmtiles(_) => None,
            #[cfg(feature = "mbtiles")]
            Self::Mbtiles(_) => None,
            #[cfg(feature = "passthrough")]
            Self::Passthrough(_) => None,
            #[cfg(feature = "geojson")]
            Self::GeoJson(_) => None,
            #[cfg(feature = "unstable-cog")]
            Self::Cog(_) => None,
            #[cfg(feature = "unstable-duckdb")]
            Self::DuckDb(_) => None,
            #[cfg(feature = "_testing")]
            Self::Test(_) => None,
            Self::DeclaredGrid(s) => s.inner().cancel_registry(),
        }
    }

    /// Zoom-level bounds for tile caching.
    #[must_use]
    pub fn cache_zoom(&self) -> CacheZoomRange {
        dispatch!(self, |s| s.cache_zoom())
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
        dispatch!(self, |s| s.get_tile(xyz, url_query).await)
    }

    /// Retrieves tile with etag for the given coordinates.
    pub async fn get_tile_with_etag(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Tile> {
        dispatch!(self, |s| s.get_tile_with_etag(xyz, url_query).await)
    }

    /// Attempts to create a fresh instance of this source.
    pub async fn try_reload(&self) -> MartinCoreResult<Arc<Self>> {
        dispatch!(self, |s| s.try_reload().await)
    }
}
