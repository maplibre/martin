use std::collections::HashMap;
use std::fmt::Debug;

use async_trait::async_trait;
use martin_tile_utils::{TileCoord, TileData, TileInfo};
use tilejson::TileJSON;

use crate::tiles::catalog::CatalogSourceEntry;
use crate::tiles::{MartinCoreResult, Tile};

/// URL query parameters for dynamic tile generation.
pub type UrlQuery = HashMap<String, String>;

/// Core trait for tile sources providing data to Martin
///
/// Implementors can serve tiles from databases, files, or other backends.
#[async_trait]
pub trait Source: Send + Sync + Debug {
    /// Unique source identifier used in URLs.
    fn get_id(&self) -> &str;

    /// `TileJSON` specification served to clients.
    fn get_tilejson(&self) -> &TileJSON;

    /// Technical tile information (format, encoding, etc.).
    fn get_tile_info(&self) -> TileInfo;

    /// Creates a boxed clone for trait object storage.
    fn clone_source(&self) -> BoxedSource;

    /// A version string for this source, if available. Default: None.
    /// If available, this string is appended to tile URLs as a query parameter,
    /// invalidating caches.
    fn get_version(&self) -> Option<String> {
        None
    }

    /// Whether this source accepts URL query parameters. Default: false.
    fn support_url_query(&self) -> bool {
        false
    }

    /// Whether martin-cp should use concurrent scraping. Default: false.
    fn benefits_from_concurrent_scraping(&self) -> bool {
        false
    }

    /// Retrieves tile data for the given coordinates.
    ///
    /// # Arguments
    /// * `xyz` - Tile coordinates (x, y, zoom)
    /// * `url_query` - Optional query parameters for dynamic tiles
    async fn get_tile(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<TileData>;

    /// Retrieves tile with etag for the given coordinates.
    ///
    /// Default implementation calls [`get_tile()`](Self::get_tile) and computes etag using `xxh3_128`.
    /// Sources can override this for more performance.
    ///
    /// # Arguments
    /// * `xyz` - Tile coordinates (x, y, zoom)
    /// * `url_query` - Optional query parameters for dynamic tiles
    async fn get_tile_with_etag(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinCoreResult<Tile> {
        let data = self.get_tile(xyz, url_query).await?;
        let etag = if data.is_empty() {
            0
        } else {
            xxhash_rust::xxh3::xxh3_128(&data)
        };
        Ok(Tile::new_with_etag(
            data,
            self.get_tile_info(),
            etag.to_string(),
        ))
    }

    /// Validates zoom level against `TileJSON` min/max zoom constraints.
    fn is_valid_zoom(&self, zoom: u8) -> bool {
        let tj = self.get_tilejson();
        tj.minzoom.is_none_or(|minzoom| zoom >= minzoom)
            && tj.maxzoom.is_none_or(|maxzoom| zoom <= maxzoom)
    }

    /// Generates catalog entry for this source.
    fn get_catalog_entry(&self) -> CatalogSourceEntry {
        let id = self.get_id();
        let tilejson = self.get_tilejson();
        let info = self.get_tile_info();
        CatalogSourceEntry {
            content_type: info.format.content_type().to_string(),
            content_encoding: info.encoding.content_encoding().map(ToString::to_string),
            name: tilejson.name.as_ref().filter(|v| *v != id).cloned(),
            description: tilejson.description.clone(),
            attribution: tilejson.attribution.clone(),
        }
    }
}

/// Boxed tile source trait object for storage in collections.
pub type BoxedSource = Box<dyn Source>;

impl Clone for BoxedSource {
    fn clone(&self) -> Self {
        self.clone_source()
    }
}
