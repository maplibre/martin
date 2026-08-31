use std::sync::Arc;

use actix_web::error::{ErrorBadRequest, ErrorNotFound};
use dashmap::DashMap;
use martin_core::tiles::catalog::TileCatalog;
use martin_core::tiles::{BoxedSource, Source};
use martin_tile_utils::TileInfo;
use tracing::debug;

use crate::config::file::ResolvedProcess;

/// Maximum number of comma-separated source ids accepted in a single
/// composite tile request (`/{source_ids}/{z}/{x}/{y}`).
const MAX_SOURCE_IDS_PER_REQUEST: usize = 128;

/// Result of resolving multiple sources for a composite tile request.
pub struct ResolvedSources {
    pub sources: Vec<(BoxedSource, ResolvedProcess)>,
    pub use_url_query: bool,
    pub info: TileInfo,
}

/// Thread-safe registry of tile sources indexed by ID.
///
/// Uses a [`DashMap`] for concurrent access without explicit locking.
/// Each source is paired with its resolved [`ResolvedProcess`].
#[derive(Default, Clone)]
pub struct TileSources(Arc<DashMap<String, (BoxedSource, ResolvedProcess)>>);

impl TileSources {
    /// Creates a new registry from flattened source collections.
    ///
    /// All sources receive the default [`ResolvedProcess`].
    #[must_use]
    pub fn new(sources: Vec<Vec<BoxedSource>>) -> Self {
        Self::new_with_process(
            sources
                .into_iter()
                .map(|group| {
                    group
                        .into_iter()
                        .map(|src| (src, ResolvedProcess::default()))
                        .collect()
                })
                .collect(),
        )
    }

    /// Creates a new registry from sources paired with their resolved process configs.
    #[must_use]
    pub fn new_with_process(sources: Vec<Vec<(BoxedSource, ResolvedProcess)>>) -> Self {
        Self(Arc::new(
            sources
                .into_iter()
                .flatten()
                .map(|(src, pc)| (src.get_id().to_owned(), (src, pc)))
                .collect(),
        ))
    }

    /// Creates a registry backed by an existing shared `DashMap`.
    #[must_use]
    pub(crate) fn from_dashmap(map: Arc<DashMap<String, (BoxedSource, ResolvedProcess)>>) -> Self {
        Self(map)
    }

    /// Returns a catalog of all sources with their metadata.
    #[must_use]
    pub fn get_catalog(&self) -> TileCatalog {
        self.0
            .iter()
            .map(|v| {
                let (src, pc) = v.value();
                let mut entry = src.get_catalog_entry();
                let info = pc.advertised_tile_info(src.get_tile_info());
                info.format
                    .content_type()
                    .clone_into(&mut entry.content_type);
                entry.content_encoding = info.encoding.compression().map(str::to_owned);
                (v.key().clone(), entry)
            })
            .collect()
    }

    /// Returns all source IDs.
    #[must_use]
    pub fn source_names(&self) -> Vec<String> {
        self.0.iter().map(|v| v.key().clone()).collect()
    }

    /// Gets a source and its process config by ID, returning 404 error if not found.
    pub fn get_source(&self, id: &str) -> actix_web::Result<(BoxedSource, ResolvedProcess)> {
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
        let ids: Vec<&str> = source_ids.split(',').collect();
        if ids.len() > MAX_SOURCE_IDS_PER_REQUEST {
            return Err(ErrorBadRequest(format!(
                "Requested {} source ids, but at most {MAX_SOURCE_IDS_PER_REQUEST} are allowed per request",
                ids.len()
            )));
        }

        let mut sources = Vec::new();
        let mut info: Option<TileInfo> = None;
        let mut use_url_query = false;

        for id in ids {
            let (src, pc) = self.get_source(id)?;
            let src_inf = pc.advertised_tile_info(src.get_tile_info());
            use_url_query |= src.support_url_query();

            // make sure all sources have the same format and encoding
            // TODO: support multiple encodings of the same format
            match info {
                Some(inf) if inf == src_inf => {}
                Some(inf) => {
                    return Err(ErrorNotFound(format!(
                        "Cannot merge sources with {inf} with {src_inf}"
                    )));
                }
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
            let tilejson = src.get_tilejson();
            debug!(
                source.id = id,
                requested.zoom = zoom,
                source.minzoom = ?tilejson.minzoom,
                source.maxzoom = ?tilejson.maxzoom,
                "Requested zoom not supported by source",
            );
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

#[cfg(test)]
mod tests {
    use tilejson::tilejson;

    use super::*;
    use crate::srv::tiles::tests::TestSource;

    fn sources_with_one_valid() -> TileSources {
        TileSources::new(vec![vec![Box::new(TestSource {
            id: "valid",
            tj: tilejson! { tiles: vec![] },
            data: vec![1, 2, 3],
            format: martin_tile_utils::Format::Mvt,
        })]])
    }

    #[test]
    fn too_many_source_ids_are_rejected() {
        let sources = sources_with_one_valid();
        let ids = vec!["valid"; MAX_SOURCE_IDS_PER_REQUEST + 1].join(",");
        assert!(sources.get_sources(&ids, None).is_err());
    }

    #[test]
    fn exactly_max_source_ids_is_not_rejected_by_the_count_check() {
        let sources = sources_with_one_valid();
        let ids = vec!["valid"; MAX_SOURCE_IDS_PER_REQUEST].join(",");
        let resolved = sources.get_sources(&ids, None).unwrap();
        assert_eq!(resolved.sources.len(), MAX_SOURCE_IDS_PER_REQUEST);
    }
}
