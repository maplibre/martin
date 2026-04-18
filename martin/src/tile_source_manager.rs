use std::sync::Arc;

use dashmap::DashMap;
use martin_core::tiles::{BoxedSource, OptTileCache};
use tracing::{info, warn};

use crate::MartinResult;
use crate::config::file::{OnInvalid, ProcessConfig};
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
    tile_sources: Arc<DashMap<String, (BoxedSource, ProcessConfig)>>,
    tile_cache: OptTileCache,
    on_invalid: OnInvalid,
}

impl TileSourceManager {
    /// Creates a new empty manager with the given cache and invalidation policy.
    #[must_use]
    pub fn new(tile_cache: OptTileCache, on_invalid: OnInvalid) -> Self {
        Self {
            tile_sources: Arc::new(DashMap::new()),
            tile_cache,
            on_invalid,
        }
    }

    /// Creates a manager pre-populated with the given sources.
    ///
    /// All sources receive the default [`ProcessConfig`].
    #[must_use]
    pub fn from_sources(
        tile_cache: OptTileCache,
        on_invalid: OnInvalid,
        sources: Vec<Vec<BoxedSource>>,
    ) -> Self {
        let with_defaults = sources
            .into_iter()
            .map(|group| {
                group
                    .into_iter()
                    .map(|src| (src, ProcessConfig::default()))
                    .collect()
            })
            .collect();
        Self::from_sources_with_process(tile_cache, on_invalid, with_defaults)
    }

    /// Creates a manager pre-populated with sources paired with their process configs.
    #[must_use]
    pub fn from_sources_with_process(
        tile_cache: OptTileCache,
        on_invalid: OnInvalid,
        sources: Vec<Vec<(BoxedSource, ProcessConfig)>>,
    ) -> Self {
        let map: DashMap<String, (BoxedSource, ProcessConfig)> = sources
            .into_iter()
            .flatten()
            .map(|(src, pc)| (src.get_id().to_string(), (src, pc)))
            .collect();
        Self {
            tile_sources: Arc::new(map),
            tile_cache,
            on_invalid,
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
    /// When a source fails to initialize, the configured [`OnInvalid`] policy
    /// controls the behavior:
    /// - [`OnInvalid::Abort`] — return the error immediately.
    /// - [`OnInvalid::Warn`] — log a warning and skip the source.
    ///
    /// Order of operations:
    /// 1. **Updates** — time-critical; invalidate cache then replace the source.
    /// 2. **Additions** — make new sources available.
    /// 3. **Removals** — garbage-collect stale sources and their cached tiles.
    pub async fn apply_changes(&self, advisory: ReloadAdvisory) -> MartinResult<()> {
        if advisory.is_empty() {
            return Ok(());
        }

        // 1. Updates: time-critical, invalidate cache then swap
        for new_source in advisory.updates {
            match new_source.source {
                Ok(src) => {
                    if let Some(cache) = &self.tile_cache {
                        cache.invalidate_source(&new_source.id);
                    }
                    self.tile_sources
                        .insert(new_source.id.clone(), (src, new_source.process));
                    info!(source.id = %new_source.id, "Updated source");
                }
                Err(err) => match self.on_invalid {
                    OnInvalid::Abort => return Err(err),
                    OnInvalid::Warn => {
                        warn!(source.id = %new_source.id, error = %err, "Skipping update");
                    }
                },
            }
        }

        // 2. Additions: make new sources available
        for new_source in advisory.additions {
            match new_source.source {
                Ok(src) => {
                    self.tile_sources
                        .insert(new_source.id.clone(), (src, new_source.process));
                    info!(source.id = %new_source.id, "Added source");
                }
                Err(err) => match self.on_invalid {
                    OnInvalid::Abort => return Err(err),
                    OnInvalid::Warn => {
                        warn!(source.id = %new_source.id, error = %err, "Skipping addition");
                    }
                },
            }
        }

        // 3. Removals: GC stale sources
        for deleted_source in &advisory.removals {
            self.tile_sources.remove(&deleted_source.id);
            if let Some(cache) = &self.tile_cache {
                cache.invalidate_source(&deleted_source.id);
            }
            info!(source.id = %deleted_source.id, "Removed source");
        }

        // 4. Flush pending cache maintenance (e.g. invalidation predicates)
        if let Some(cache) = &self.tile_cache {
            cache.run_pending_tasks().await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use insta::assert_yaml_snapshot;
    use martin_core::CacheZoomRange;
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
        fn cache_zoom(&self) -> CacheZoomRange {
            CacheZoomRange::default()
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
        let cache = TileCache::new(1024 * 1024, None, None); // 1 MB
        TileSourceManager::new(Some(cache), OnInvalid::Abort)
    }

    fn new_source(name: &str) -> NewSource {
        NewSource {
            id: name.to_string(),
            source: Ok(Box::new(TestSource {
                id: name.to_string(),
                tj: tilejson! { tiles: vec![] },
            })),
            process: ProcessConfig::default(),
        }
    }

    fn sorted_source_names(mgr: &TileSourceManager) -> Vec<String> {
        let mut names = mgr.tile_sources().source_names();
        names.sort();
        names
    }

    #[tokio::test]
    async fn apply_additions() {
        let mgr = make_manager();
        let advisory = ReloadAdvisory {
            additions: vec![new_source("src_a"), new_source("src_b")],
            ..Default::default()
        };
        mgr.apply_changes(advisory).await.unwrap();
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"
        - src_a
        - src_b
        ");
    }

    #[tokio::test]
    async fn apply_removals() {
        let mgr = make_manager();
        let add = ReloadAdvisory {
            additions: vec![new_source("src_a"), new_source("src_b")],
            ..Default::default()
        };
        mgr.apply_changes(add).await.unwrap();
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"
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
        mgr.apply_changes(remove).await.unwrap();
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"- src_b");
    }

    #[tokio::test]
    async fn apply_updates() {
        let mgr = make_manager();
        let add = ReloadAdvisory {
            additions: vec![new_source("src_a")],
            ..Default::default()
        };
        mgr.apply_changes(add).await.unwrap();

        let update = ReloadAdvisory {
            updates: vec![new_source("src_a")],
            ..Default::default()
        };
        mgr.apply_changes(update).await.unwrap();
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"- src_a");
    }

    #[tokio::test]
    async fn empty_advisory_is_noop() {
        let mgr = make_manager();
        mgr.apply_changes(ReloadAdvisory::default()).await.unwrap();
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"[]");
    }

    #[test]
    fn from_sources_populates_map() {
        let src = Box::new(TestSource {
            id: "x".to_string(),
            tj: tilejson! { tiles: vec![] },
        }) as BoxedSource;
        let mgr = TileSourceManager::from_sources(None, OnInvalid::Abort, vec![vec![src]]);
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"- x");
        assert!(mgr.tile_cache().is_none());
    }

    #[tokio::test]
    async fn apply_changes_without_cache() {
        let mgr = TileSourceManager::new(None, OnInvalid::Abort);
        let advisory = ReloadAdvisory {
            additions: vec![new_source("a")],
            ..Default::default()
        };
        mgr.apply_changes(advisory).await.unwrap();
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"- a");
    }

    /// Regression test for <https://github.com/maplibre/martin/discussions/2767>:
    /// a persistently-failing source must not block later additions or removals
    /// from reaching the live catalog when `OnInvalid::Warn` is in effect.
    ///
    /// Simulates the directory-watcher loop:
    /// 1. Watch a directory with one bad file → catalog empty.
    /// 2. Add a valid file → catalog gains it.
    /// 3. Delete the valid file → catalog drops it.
    #[tokio::test]
    async fn watcher_loop_around_persistent_bad_file() {
        use std::collections::BTreeMap;
        use std::path::{Path, PathBuf};

        use tempfile::TempDir;

        use crate::MartinError;
        use crate::config::file::{ConfigFileError, ProcessConfig};

        const BAD_PREFIX: &str = "bad_";

        fn scan(dir: &Path) -> BTreeMap<String, u64> {
            let mut out = BTreeMap::new();
            for entry in std::fs::read_dir(dir).expect("read tempdir").flatten() {
                if !entry.file_type().is_ok_and(|t| t.is_file()) {
                    continue;
                }
                let id = entry
                    .path()
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(str::to_string)
                    .expect("file stem is valid utf-8");
                out.insert(id, 1);
            }
            out
        }

        #[expect(
            clippy::unused_async,
            reason = "must satisfy AsyncFn for ReloadAdvisory::from_maps"
        )]
        async fn build(id: String, dir: PathBuf) -> MartinResult<BoxedSource> {
            if id.starts_with(BAD_PREFIX) {
                return Err(MartinError::from(ConfigFileError::InvalidFilePath(
                    dir.join(format!("{id}.tiles")),
                )));
            }
            Ok(Box::new(TestSource {
                id,
                tj: tilejson! { tiles: vec![] },
            }))
        }

        let dir = TempDir::new().expect("create tempdir");
        let dir_path = dir.path().to_path_buf();
        let mgr = TileSourceManager::new(None, OnInvalid::Warn);
        let mut state: BTreeMap<String, u64> = BTreeMap::new();

        // Reconciles the manager with the current on-disk state and advances `state`.
        let tick = async |state: &mut BTreeMap<String, u64>| {
            let next = scan(&dir_path);
            let advisory = ReloadAdvisory::from_maps(
                state,
                &next,
                async |id| build(id, dir_path.clone()).await,
                ProcessConfig::default(),
            )
            .await;
            mgr.apply_changes(advisory)
                .await
                .expect("warn policy must not abort on a bad file");
            *state = next;
        };

        std::fs::write(dir.path().join("bad_a.tiles"), b"").unwrap();
        tick(&mut state).await;
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"[]");

        std::fs::write(dir.path().join("good_x.tiles"), b"").unwrap();
        tick(&mut state).await;
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"- good_x");

        std::fs::remove_file(dir.path().join("good_x.tiles")).unwrap();
        tick(&mut state).await;
        assert_yaml_snapshot!(sorted_source_names(&mgr), @"[]");
    }
}
