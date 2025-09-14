use std::collections::HashMap;
use std::fmt::Debug;

use actix_web::error::ErrorNotFound;
use async_trait::async_trait;
use dashmap::DashMap;
use log::debug;
use martin_core::tiles::catalog::{CatalogSourceEntry, TileCatalog};
pub use martin_tile_utils::TileData;
use martin_tile_utils::{TileCoord, TileInfo};
use tilejson::TileJSON;

use crate::MartinResult;
pub type UrlQuery = HashMap<String, String>;

pub type TileInfoSource = Box<dyn Source>;

impl Clone for TileInfoSource {
    fn clone(&self) -> Self {
        self.clone_source()
    }
}

#[derive(Default, Clone)]
pub struct TileSources(DashMap<String, TileInfoSource>);

impl TileSources {
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

    #[must_use]
    pub fn get_catalog(&self) -> TileCatalog {
        self.0
            .iter()
            .map(|v| (v.key().to_string(), v.get_catalog_entry()))
            .collect()
    }

    #[must_use]
    pub fn source_names(&self) -> Vec<String> {
        self.0.iter().map(|v| v.key().to_string()).collect()
    }

    pub fn get_source(&self, id: &str) -> actix_web::Result<TileInfoSource> {
        Ok(self
            .0
            .get(id)
            .ok_or_else(|| ErrorNotFound(format!("Source {id} does not exist")))?
            .value()
            .clone())
    }

    /// Get a list of sources, and the tile info for the merged sources.
    /// Ensure that all sources have the same format and encoding.
    /// If `zoom` is specified, filter out sources that do not support it.
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

    #[must_use]
    pub fn check_zoom(src: &dyn Source, id: &str, zoom: u8) -> bool {
        let is_valid = src.is_valid_zoom(zoom);
        if !is_valid {
            debug!("Zoom {zoom} is not valid for source {id}");
        }
        is_valid
    }

    /// Whether this [`Source`] benefits from concurrency when being scraped via `martin-cp`.
    ///
    /// If this returns `true`, martin-cp will suggest concurrent scraping.
    #[must_use]
    pub fn benefits_from_concurrent_scraping(&self) -> bool {
        self.0.iter().any(|s| s.benefits_from_concurrent_scraping())
    }
}

#[async_trait]
pub trait Source: Send + Debug {
    /// ID under which this [`Source`] is identified if accessed externally
    fn get_id(&self) -> &str;

    /// `TileJSON` of this [`Source`]
    ///
    /// Will be communicated verbatim to the outside to give rendering engines information about the source's contents such as zoom levels, center points, ...
    fn get_tilejson(&self) -> &TileJSON;

    /// Information for serving the source such as which Mime-type to apply or how compression should work
    fn get_tile_info(&self) -> TileInfo;

    fn clone_source(&self) -> TileInfoSource;

    fn support_url_query(&self) -> bool {
        false
    }
    /// Whether this [`Source`] benefits from concurrency when being scraped via `martin-cp`.
    ///
    /// If this returns `true`, martin-cp will suggest concurrent scraping.
    fn benefits_from_concurrent_scraping(&self) -> bool {
        false
    }

    async fn get_tile(
        &self,
        xyz: TileCoord,
        url_query: Option<&UrlQuery>,
    ) -> MartinResult<TileData>;

    fn is_valid_zoom(&self, zoom: u8) -> bool {
        let tj = self.get_tilejson();
        tj.minzoom.is_none_or(|minzoom| zoom >= minzoom)
            && tj.maxzoom.is_none_or(|maxzoom| zoom <= maxzoom)
    }

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
