use std::sync::Arc;

use dashmap::DashMap;
use martin_core::tiles::{BoxedSource, OptTileCache};
use tracing::info;

use crate::reload::ReloadAdvisory;
use crate::source::TileSources;

/// Manages the live set of tile sources and their caches.
///
/// A broad lock is not needed because each reloader manages a mutually exclusive
/// set of sources, and [`DashMap`] provides atomic per-key operations.
///
/// `TileCache` has an inner `Arc`, so the whole
/// `TileSourceManager` is cheap to clone.
#[derive(Clone)]
pub struct TileSourceManager {
    tile_sources: Arc<DashMap<String, BoxedSource>>,
    tile_cache: OptTileCache,
}

impl TileSourceManager {
    /// Creates a new empty manager with the given cache and resolver.
    #[must_use]
    pub fn new(tile_cache: OptTileCache) -> Self {
        Self {
            tile_sources: Arc::new(DashMap::new()),
            tile_cache,
        }
    }

    /// Creates a manager pre-populated with the given sources.
    #[must_use]
    pub fn from_sources(tile_cache: OptTileCache, sources: Vec<Vec<BoxedSource>>) -> Self {
        let map: DashMap<String, BoxedSource> = sources
            .into_iter()
            .flatten()
            .map(|src| (src.get_id().to_string(), src))
            .collect();
        Self {
            tile_sources: Arc::new(map),
            tile_cache,
        }
    }

    /// Returns a [`TileSources`] view for read-only tile serving.
    #[must_use]
    pub fn tile_sources(&self) -> TileSources {
        TileSources::from_dashmap(self.tile_sources.clone())
    }

    /// Returns a reference to the optional tile cache.
    #[must_use]
    pub fn tile_cache(&self) -> &OptTileCache {
        &self.tile_cache
    }

    /// Applies a [`ReloadAdvisory`] to the live source set.
    ///
    /// Order of operations:
    /// 1. **Updates** — time-critical; invalidate cache then replace the source.
    /// 2. **Additions** — make new sources available.
    /// 3. **Removals** — garbage-collect stale sources and their cached tiles.
    pub fn apply_changes(&self, advisory: ReloadAdvisory) {
        if advisory.is_empty() {
            return;
        }

        // 1. Updates: time-critical, invalidate cache then swap
        for new_source in advisory.updates {
            let Some(src) = new_source.source else {
                tracing::warn!(
                    "Skipping update for {:?}: source failed to initialize",
                    new_source.id
                );
                continue;
            };
            if let Some(cache) = &self.tile_cache {
                cache.invalidate_source(&new_source.id);
            }
            self.tile_sources.insert(new_source.id.clone(), src);
            info!("Updated source: {:?}", new_source.id);
        }

        // 2. Additions: make new sources available
        for new_source in advisory.additions {
            let Some(src) = new_source.source else {
                tracing::warn!(
                    "Skipping addition of {:?}: source failed to initialize",
                    new_source.id
                );
                continue;
            };
            self.tile_sources.insert(new_source.id.clone(), src);
            info!("Added source: {:?}", new_source.id);
        }

        // 3. Removals: GC stale sources
        for deleted_source in &advisory.removals {
            self.tile_sources.remove(&deleted_source.id);
            if let Some(cache) = &self.tile_cache {
                cache.invalidate_source(&deleted_source.id);
            }
            info!("Removed source: {:?}", deleted_source.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use insta::assert_yaml_snapshot;
    use martin_core::tiles::{MartinCoreResult, Source, TileCache, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::{TileJSON, tilejson};

    use super::*;
    use crate::reload::{DeletedSource, NewSource};

    #[derive(Debug, Clone)]
    struct TestSource {
        id: String,
        tj: TileJSON,
    }

    #[async_trait]
    impl Source for TestSource {
        fn get_id(&self) -> &str {
            &self.id
        }
        fn get_tilejson(&self) -> &TileJSON {
            &self.tj
        }
        fn get_tile_info(&self) -> TileInfo {
            TileInfo::new(Format::Mvt, Encoding::Uncompressed)
        }
        fn clone_source(&self) -> BoxedSource {
            Box::new(self.clone())
        }
        async fn get_tile(
            &self,
            _xyz: TileCoord,
            _url_query: Option<&UrlQuery>,
        ) -> MartinCoreResult<TileData> {
            Ok(vec![1, 2, 3])
        }
    }

    fn make_manager() -> TileSourceManager {
        let cache = TileCache::new(1024 * 1024); // 1 MB
        TileSourceManager::new(Some(cache))
    }

    fn new_source(name: &str) -> NewSource {
        NewSource {
            id: name.to_string(),
            source: Some(Box::new(TestSource {
                id: name.to_string(),
                tj: tilejson! { tiles: vec![] },
            })),
        }
    }

    fn sorted_source_names(mgr: &TileSourceManager) -> Vec<String> {
        let mut names = mgr.tile_sources().source_names();
        names.sort();
        names
    }

    #[test]
    fn apply_additions() {
        let mgr = make_manager();
        let advisory = ReloadAdvisory {
            additions: vec![new_source("src_a"), new_source("src_b")],
            ..Default::default()
        };
        mgr.apply_changes(advisory);
        assert_yaml_snapshot!(sorted_source_names(&mgr), @r"
        - src_a
        - src_b
        ");
    }

    #[test]
    fn apply_removals() {
        let mgr = make_manager();
        let add = ReloadAdvisory {
            additions: vec![new_source("src_a"), new_source("src_b")],
            ..Default::default()
        };
        mgr.apply_changes(add);
        assert_yaml_snapshot!(sorted_source_names(&mgr), @r"
        - src_a
        - src_b
        ");

        let mut removals = std::collections::BTreeSet::new();
        removals.insert(DeletedSource {
            id: "src_a".to_string(),
        });
        let remove = ReloadAdvisory {
            removals,
            ..Default::default()
        };
        mgr.apply_changes(remove);
        assert_yaml_snapshot!(sorted_source_names(&mgr), @r"
        - src_b
        ");
    }

    #[test]
    fn apply_updates() {
        let mgr = make_manager();
        let add = ReloadAdvisory {
            additions: vec![new_source("src_a")],
            ..Default::default()
        };
        mgr.apply_changes(add);

        let update = ReloadAdvisory {
            updates: vec![new_source("src_a")],
            ..Default::default()
        };
        mgr.apply_changes(update);
        assert_yaml_snapshot!(sorted_source_names(&mgr), @r"
        - src_a
        ");
    }

    #[test]
    fn empty_advisory_is_noop() {
        let mgr = make_manager();
        mgr.apply_changes(ReloadAdvisory::default());
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"[]");
    }

    #[test]
    fn from_sources_populates_map() {
        let src = Box::new(TestSource {
            id: "x".to_string(),
            tj: tilejson! { tiles: vec![] },
        }) as BoxedSource;
        let mgr = TileSourceManager::from_sources(None, vec![vec![src]]);
        assert_yaml_snapshot!(sorted_source_names(&mgr), @r"
        - x
        ");
        assert!(mgr.tile_cache().is_none());
    }

    #[test]
    fn apply_changes_without_cache() {
        let mgr = TileSourceManager::new(None);
        let advisory = ReloadAdvisory {
            additions: vec![new_source("a")],
            ..Default::default()
        };
        mgr.apply_changes(advisory);
        assert_yaml_snapshot!(sorted_source_names(&mgr), @r"
        - a
        ");
    }
}
