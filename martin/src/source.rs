use actix_web::error::ErrorNotFound;
use dashmap::DashMap;
use tracing::debug;
use martin_core::tiles::catalog::TileCatalog;
use martin_core::tiles::{BoxedSource, Source};
use martin_tile_utils::TileInfo;

/// Thread-safe registry of tile sources indexed by ID.
///
/// Uses a [`DashMap`] for concurrent access without explicit locking.
#[derive(Default, Clone)]
pub struct TileSources(DashMap<String, BoxedSource>);

impl TileSources {
    /// Creates a new registry from flattened source collections.
    #[must_use]
    pub fn new(sources: Vec<Vec<BoxedSource>>) -> Self {
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
            .map(|v| (v.key().clone(), v.get_catalog_entry()))
            .collect()
    }

    /// Returns all source IDs.
    #[must_use]
    pub fn source_names(&self) -> Vec<String> {
        self.0.iter().map(|v| v.key().clone()).collect()
    }

    /// Gets a source by ID, returning 404 error if not found.
    pub fn get_source(&self, id: &str) -> actix_web::Result<BoxedSource> {
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
    ) -> actix_web::Result<(Vec<BoxedSource>, bool, TileInfo)> {
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
