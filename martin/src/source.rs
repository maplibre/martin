use std::sync::Arc;

use dashmap::DashMap;
use martin_core::tiles::catalog::{CatalogSourceEntry, TileCatalog};
use martin_core::tiles::{BoxedSource, Source};
use martin_tile_utils::TileInfo;
use tracing::{debug, info};

use crate::config::file::ResolvedProcess;
use crate::srv::TileError;

/// Maximum number of comma-separated source ids accepted in a single
/// composite tile request (`/{source_ids}/{z}/{x}/{y}`).
const MAX_SOURCE_IDS_PER_REQUEST: usize = 128;

/// Result of resolving multiple sources for a composite tile request.
pub struct ResolvedSources {
    pub sources: Vec<(BoxedSource, ResolvedProcess)>,
    pub use_url_query: bool,
    pub info: TileInfo,
}

/// Errors from registering a tile source alias.
#[derive(Debug, thiserror::Error)]
pub enum TileAliasError {
    /// The alias name cannot be requested.
    #[error(
        "Tile source alias {0:?} is invalid: alias names must be non-empty and must not contain commas"
    )]
    InvalidAliasName(String),

    /// The alias lists no sources.
    #[error("Tile source alias {0:?} does not reference any tile sources")]
    EmptyAlias(String),

    /// The alias references more sources than a single request may use.
    #[error(
        "Tile source alias {alias:?} references {requested} tile sources, but at most {max} are allowed"
    )]
    TooManySourcesInAlias {
        /// Alias name.
        alias: String,
        /// Referenced source count.
        requested: usize,
        /// Allowed maximum.
        max: usize,
    },

    /// The alias references another alias instead of a tile source.
    #[error(
        "Tile source alias {alias:?} references {source_id:?}, which is itself an alias; aliases may only reference tile sources"
    )]
    AliasWithinAlias {
        /// Alias name.
        alias: String,
        /// The referenced alias.
        source_id: String,
    },

    /// The alias references a tile source that is not registered.
    #[error("Tile source alias {alias:?} references unknown tile source {source_id:?}")]
    AliasSourceNotFound {
        /// Alias name.
        alias: String,
        /// The referenced tile source.
        source_id: String,
    },
}

/// Thread-safe registry of tile sources indexed by ID.
///
/// Uses a [`DashMap`] for concurrent access without explicit locking.
/// Each source is paired with its resolved [`ResolvedProcess`].
#[derive(Default, Clone)]
pub struct TileSources {
    sources: Arc<DashMap<String, (BoxedSource, ResolvedProcess)>>,
    /// Map of alias name to the tile source ids it combines.
    aliases: Arc<DashMap<String, Vec<String>>>,
}

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
        Self {
            sources: Arc::new(
                sources
                    .into_iter()
                    .flatten()
                    .map(|(src, pc)| (src.get_id().to_owned(), (src, pc)))
                    .collect(),
            ),
            aliases: Arc::default(),
        }
    }

    /// Creates a registry backed by existing shared maps.
    #[must_use]
    pub(crate) fn from_maps(
        sources: Arc<DashMap<String, (BoxedSource, ResolvedProcess)>>,
        aliases: Arc<DashMap<String, Vec<String>>>,
    ) -> Self {
        Self { sources, aliases }
    }

    /// Returns a catalog of all sources and aliases with their metadata.
    ///
    /// An alias is listed under its own name with the format its member sources serve.
    #[must_use]
    pub fn get_catalog(&self) -> TileCatalog {
        let mut catalog: TileCatalog = self
            .sources
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
            .collect();
        for alias in self.aliases.iter() {
            let Some(member) = alias.value().iter().find_map(|id| self.sources.get(id)) else {
                continue;
            };
            let (src, pc) = member.value();
            let info = pc.advertised_tile_info(src.get_tile_info());
            let entry = CatalogSourceEntry {
                content_type: info.format.content_type().to_owned(),
                content_encoding: info.encoding.compression().map(str::to_owned),
                ..CatalogSourceEntry::default()
            };
            catalog.insert(alias.key().clone(), entry);
        }
        catalog
    }

    /// Returns all source IDs.
    #[must_use]
    pub fn source_names(&self) -> Vec<String> {
        self.sources.iter().map(|v| v.key().clone()).collect()
    }

    /// Registers a named combination of tile sources that serves like a single source.
    ///
    /// Every member must name a registered tile source, not another alias.
    /// An alias may share the name of a source it references.
    /// Requests for such a name serve the alias.
    pub fn add_alias(&self, name: String, sources: Vec<String>) -> Result<(), TileAliasError> {
        if name.is_empty() || name.contains(',') {
            return Err(TileAliasError::InvalidAliasName(name));
        }
        if sources.is_empty() {
            return Err(TileAliasError::EmptyAlias(name));
        }
        if sources.len() > MAX_SOURCE_IDS_PER_REQUEST {
            return Err(TileAliasError::TooManySourcesInAlias {
                alias: name,
                requested: sources.len(),
                max: MAX_SOURCE_IDS_PER_REQUEST,
            });
        }
        for source in &sources {
            if self.aliases.contains_key(source) {
                return Err(TileAliasError::AliasWithinAlias {
                    alias: name,
                    source_id: source.clone(),
                });
            }
            if !self.sources.contains_key(source) {
                return Err(TileAliasError::AliasSourceNotFound {
                    alias: name,
                    source_id: source.clone(),
                });
            }
        }
        if self.sources.contains_key(&name) {
            info!(
                source.alias = %name,
                "Tile source alias shadows a tile source of the same name; requests for it will serve the alias"
            );
        }
        info!(
            source.alias = %name,
            source.ids = %sources.join(", "),
            "Configured tile source alias"
        );
        self.aliases.insert(name, sources);
        Ok(())
    }

    /// Splits a request id list and replaces every alias with its member sources, in order.
    #[must_use]
    pub fn expand_ids(&self, source_ids: &str) -> Vec<String> {
        let mut expanded = Vec::new();
        for id in source_ids.split(',') {
            if let Some(alias) = self.aliases.get(id) {
                expanded.extend(alias.value().iter().cloned());
            } else {
                expanded.push(id.to_owned());
            }
        }
        expanded
    }

    /// Gets a source and its process config by ID, returning 404 error if not found.
    pub fn get_source(&self, id: &str) -> Result<(BoxedSource, ResolvedProcess), TileError> {
        Ok(self
            .sources
            .get(id)
            .ok_or_else(|| TileError::UnknownSource(id.to_owned()))?
            .value()
            .clone())
    }

    /// Gets multiple sources for composite tiles, ensuring format compatibility.
    ///
    /// Parses comma-separated source IDs, replaces aliases with their member sources,
    /// and validates all sources have matching format/encoding.
    /// Optionally filters by zoom level support.
    ///
    #[hotpath::measure]
    pub fn get_sources(
        &self,
        source_ids: &str,
        zoom: Option<u8>,
    ) -> Result<ResolvedSources, TileError> {
        let ids: Vec<&str> = source_ids.split(',').collect();
        if ids.len() > MAX_SOURCE_IDS_PER_REQUEST {
            return Err(TileError::TooManySources {
                requested: ids.len(),
                max: MAX_SOURCE_IDS_PER_REQUEST,
            });
        }

        let mut sources = Vec::new();
        let mut info: Option<TileInfo> = None;
        let mut use_url_query = false;

        for id in ids {
            if let Some(alias) = self.aliases.get(id) {
                for member in alias.value() {
                    self.collect_source(member, zoom, &mut sources, &mut info, &mut use_url_query)?;
                }
            } else {
                self.collect_source(id, zoom, &mut sources, &mut info, &mut use_url_query)?;
            }
        }

        Ok(ResolvedSources {
            sources,
            use_url_query,
            info: info.expect("at least one source must be present"),
        })
    }

    /// Adds one source to a composite resolution, checking its format against the others.
    fn collect_source(
        &self,
        id: &str,
        zoom: Option<u8>,
        sources: &mut Vec<(BoxedSource, ResolvedProcess)>,
        info: &mut Option<TileInfo>,
        use_url_query: &mut bool,
    ) -> Result<(), TileError> {
        let (src, pc) = self.get_source(id)?;
        let src_inf = pc.advertised_tile_info(src.get_tile_info());
        *use_url_query |= src.support_url_query();

        // make sure all sources have the same format and encoding
        // TODO: support multiple encodings of the same format
        match *info {
            Some(inf) if inf == src_inf => {}
            Some(inf) => {
                return Err(TileError::MismatchedSources {
                    left: inf,
                    right: src_inf,
                });
            }
            None => *info = Some(src_inf),
        }

        // TODO: Use chained-if-let once available
        if match zoom {
            Some(zoom) if Self::check_zoom(&*src, id, zoom) => true,
            None => true,
            _ => false,
        } {
            sources.push((src, pc));
        }
        Ok(())
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
        self.sources
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

    #[test]
    fn an_alias_expands_to_its_member_sources_in_order() {
        let sources = sources_with_one_valid();
        sources
            .add_alias(
                "both".to_owned(),
                vec!["valid".to_owned(), "valid".to_owned()],
            )
            .unwrap();
        assert_eq!(
            sources.expand_ids("both,valid"),
            ["valid", "valid", "valid"]
        );
        let resolved = sources.get_sources("both,valid", None).unwrap();
        assert_eq!(resolved.sources.len(), 3);
    }

    #[test]
    fn an_alias_must_name_registered_sources_and_never_another_alias() {
        let sources = sources_with_one_valid();
        assert!(matches!(
            sources.add_alias("cities".to_owned(), vec!["missing".to_owned()]),
            Err(TileAliasError::AliasSourceNotFound { .. })
        ));
        sources
            .add_alias("cities".to_owned(), vec!["valid".to_owned()])
            .unwrap();
        assert!(matches!(
            sources.add_alias("nested".to_owned(), vec!["cities".to_owned()]),
            Err(TileAliasError::AliasWithinAlias { .. })
        ));
    }
}
