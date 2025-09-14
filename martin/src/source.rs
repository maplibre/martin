use std::collections::HashMap;
use std::fmt::Debug;

use actix_web::error::ErrorNotFound;
use async_trait::async_trait;
use dashmap::DashMap;
use log::debug;
use martin_core::tiles::{
    MartinCoreResult,
    catalog::{CatalogSourceEntry, TileCatalog},
};
pub use martin_tile_utils::TileData;
use martin_tile_utils::{TileCoord, TileInfo};
use tilejson::TileJSON;

/// URL query parameters for dynamic tile generation.
pub type UrlQuery = HashMap<String, String>;

/// Boxed tile source trait object for storage in collections.
pub type TileInfoSource = Box<dyn Source>;

impl Clone for TileInfoSource {
    fn clone(&self) -> Self {
        self.clone_source()
    }
}

/// Thread-safe registry of tile sources indexed by ID.
///
/// Uses a [`DashMap`] for concurrent access without explicit locking.
#[derive(Default, Clone)]
pub struct TileSources(DashMap<String, TileInfoSource>);

impl TileSources {
    /// Creates a new registry from flattened source collections.
    #[must_use]
    pub fn new(sources: Vec<Vec<TileInfoSource>>) -> Self {
        Self(
            sources
                .into_iter()
                .flatten()
                .map(|src| (src.get_id().to_string(), src))
                .collect(),
        )
    }

    /// Returns a catalog of all sources with their metadata.
    #[must_use]
    pub fn get_catalog(&self) -> TileCatalog {
        self.0
            .iter()
            .map(|v| (v.key().to_string(), v.get_catalog_entry()))
            .collect()
    }

    /// Returns all source IDs.
    #[must_use]
    pub fn source_names(&self) -> Vec<String> {
        self.0.iter().map(|v| v.key().to_string()).collect()
    }

    /// Gets a source by ID, returning 404 error if not found.
    pub fn get_source(&self, id: &str) -> actix_web::Result<TileInfoSource> {
        Ok(self
            .0
            .get(id)
            .ok_or_else(|| ErrorNotFound(format!("Source {id} does not exist")))?
            .value()
            .clone())
    }

    /// Gets multiple sources for composite tiles, ensuring format compatibility.
    ///
    /// Parses comma-separated source IDs and validates all sources have matching
    /// format/encoding. Optionally filters by zoom level support.
    ///
    /// Returns (`sources`, `supports_url_query`, `merged_tile_info`).
    pub fn get_sources(
        &self,
        source_ids: &str,
        zoom: Option<u8>,
    ) -> actix_web::Result<(Vec<TileInfoSource>, bool, TileInfo)> {
        let mut sources = Vec::new();
        let mut info: Option<TileInfo> = None;
        let mut use_url_query = false;

        for id in source_ids.split(',') {
            let src = self.get_source(id)?;
            let src_inf = src.get_tile_info();
            use_url_query |= src.support_url_query();

            // make sure all sources have the same format and encoding
            // TODO: support multiple encodings of the same format
            match info {
                Some(inf) if inf == src_inf => {}
                Some(inf) => Err(ErrorNotFound(format!(
                    "Cannot merge sources with {inf} with {src_inf}"
                )))?,
                None => info = Some(src_inf),
            }

            // TODO: Use chained-if-let once available
            if match zoom {
                Some(zoom) if Self::check_zoom(&*src, id, zoom) => true,
                None => true,
                _ => false,
            } {
                sources.push(src);
            }
        }

        // format is guaranteed to be Some() here
        Ok((sources, use_url_query, info.unwrap()))
    }

    /// Validates zoom level support for a source
    #[must_use]
    pub fn check_zoom(src: &dyn Source, id: &str, zoom: u8) -> bool {
        let is_valid = src.is_valid_zoom(zoom);
        if !is_valid {
            debug!("Zoom {zoom} is not valid for source {id}");
        }
        is_valid
    }

    /// Returns if any source benefits from concurrent scraping by martin-cp
    #[must_use]
    pub fn benefits_from_concurrent_scraping(&self) -> bool {
        self.0.iter().any(|s| s.benefits_from_concurrent_scraping())
    }
}

/// Core trait for tile sources providing data to Martin
///
/// Implementors can serve tiles from databases, files, or other backends.
#[async_trait]
pub trait Source: Send + Debug {
    /// Unique source identifier used in URLs.
    fn get_id(&self) -> &str;

    /// `TileJSON` specification served to clients.
    fn get_tilejson(&self) -> &TileJSON;

    /// Technical tile information (format, encoding, etc.).
    fn get_tile_info(&self) -> TileInfo;

    /// Creates a boxed clone for trait object storage.
    fn clone_source(&self) -> TileInfoSource;

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
