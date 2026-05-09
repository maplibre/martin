use std::sync::Arc;

use actix_web::error::ErrorNotFound;
use dashmap::DashMap;
use martin_core::tiles::catalog::TileCatalog;
use martin_core::tiles::{BoxedSource, Source};
use martin_tile_utils::TileInfo;
use tracing::debug;

use crate::config::file::ProcessConfig;

/// Result of resolving multiple sources for a composite tile request.
pub struct ResolvedSources {
    pub sources: Vec<(BoxedSource, ProcessConfig)>,
    pub use_url_query: bool,
    pub info: TileInfo,
}

/// Thread-safe registry of tile sources indexed by ID.
///
/// Uses a [`DashMap`] for concurrent access without explicit locking.
/// Each source is paired with its resolved [`ProcessConfig`].
#[derive(Default, Clone)]
pub struct TileSources(Arc<DashMap<String, (BoxedSource, ProcessConfig)>>);

impl TileSources {
    /// Creates a new registry from flattened source collections.
    ///
    /// All sources receive the default [`ProcessConfig`].
    #[must_use]
    pub fn new(sources: Vec<Vec<BoxedSource>>) -> Self {
        Self::new_with_process(
            sources
                .into_iter()
                .map(|group| {
                    group
                        .into_iter()
                        .map(|src| (src, ProcessConfig::default()))
                        .collect()
                })
                .collect(),
        )
    }

    /// Creates a new registry from sources paired with their resolved process configs.
    #[must_use]
    pub fn new_with_process(sources: Vec<Vec<(BoxedSource, ProcessConfig)>>) -> Self {
        Self(Arc::new(
            sources
                .into_iter()
                .flatten()
                .map(|(src, pc)| (src.get_id().to_string(), (src, pc)))
                .collect(),
        ))
    }

    /// Creates a registry backed by an existing shared `DashMap`.
    #[must_use]
    pub(crate) fn from_dashmap(map: Arc<DashMap<String, (BoxedSource, ProcessConfig)>>) -> Self {
        Self(map)
    }

    /// Returns a catalog of all sources with their metadata.
    #[must_use]
    pub fn get_catalog(&self) -> TileCatalog {
        self.0
            .iter()
            .map(|v| {
                let (src, _pc) = v.value();
                (v.key().clone(), src.get_catalog_entry())
            })
            .collect()
    }

    /// Returns all source IDs.
    #[must_use]
    pub fn source_names(&self) -> Vec<String> {
        self.0.iter().map(|v| v.key().clone()).collect()
    }

    /// Gets a source and its process config by ID, returning 404 error if not found.
    pub fn get_source(&self, id: &str) -> actix_web::Result<(BoxedSource, ProcessConfig)> {
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
    #[hotpath::measure]
    pub fn get_sources(
        &self,
        source_ids: &str,
        zoom: Option<u8>,
    ) -> actix_web::Result<ResolvedSources> {
        let mut sources = Vec::new();
        let mut info: Option<TileInfo> = None;
        let mut use_url_query = false;

        for id in source_ids.split(',') {
            let (src, pc) = self.get_source(id)?;
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
                sources.push((src, pc));
            }
        }

        Ok(ResolvedSources {
            sources,
            use_url_query,
            info: info.expect("at least one source must be present"),
        })
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
        self.0
            .iter()
            .any(|s| s.value().0.benefits_from_concurrent_scraping())
    }
}
