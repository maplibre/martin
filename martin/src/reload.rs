use std::collections::{BTreeMap, BTreeSet};

use martin_core::tiles::BoxedSource;

use crate::MartinResult;

/// A source to be added or updated in the [`TileSourceManager`](super::TileSourceManager).
#[derive(Debug)]
pub struct NewSource {
    /// Resolved source ID.
    pub id: String,
    /// The tile source implementation, or an error if initialization failed.
    pub source: MartinResult<BoxedSource>,
}

/// A source to be removed from the [`TileSourceManager`](super::TileSourceManager).
#[derive(Debug, Ord, PartialOrd, PartialEq, Eq)]
pub struct DeletedSource {
    /// Resolved source ID.
    pub id: String,
}

/// Describes a set of changes to be applied to the source catalog.
///
/// Built by reloaders via [`from_sets`](Self::from_sets) (unversioned sources)
/// or [`from_maps`](Self::from_maps) (versioned sources), then handed to
/// [`TileSourceManager::apply_changes`](super::TileSourceManager::apply_changes).
#[derive(Default, Debug)]
pub struct ReloadAdvisory {
    pub additions: Vec<NewSource>,
    pub updates: Vec<NewSource>,
    pub removals: BTreeSet<DeletedSource>,
}

impl ReloadAdvisory {
    /// Generates an advisory for **unversioned** sources (e.g., Postgres, `PMTiles`).
    ///
    /// Any source that disappears is a removal; any source that appears is an addition.
    /// Sources that remain are left untouched (no update concept without versions).
    pub async fn from_sets<F>(
        previous_ids: &BTreeSet<String>,
        next_ids: &BTreeSet<String>,
        initializer: F,
    ) -> Self
    where
        F: AsyncFn(String) -> MartinResult<BoxedSource>,
    {
        let removals = previous_ids
            .difference(next_ids)
            .map(|id| DeletedSource { id: id.clone() })
            .collect();

        let mut additions: Vec<NewSource> = vec![];
        for id in next_ids.difference(previous_ids) {
            additions.push(NewSource {
                id: id.clone(),
                source: initializer(id.clone()).await,
            });
        }

        Self {
            additions,
            updates: Vec::new(),
            removals,
        }
    }

    /// Generates an advisory for **versioned** sources (e.g., `MBTiles`, COG).
    ///
    /// Compares keys and version values to distinguish between additions, removals,
    /// and updates (version changed).
    pub async fn from_maps<F, V: Eq + Copy>(
        previous_map: &BTreeMap<String, V>,
        next_map: &BTreeMap<String, V>,
        initializer: F,
    ) -> Self
    where
        F: AsyncFn(String) -> MartinResult<BoxedSource>,
    {
        let mut advisory = Self::default();

        for id in previous_map.keys() {
            if !next_map.contains_key(id) {
                advisory.removals.insert(DeletedSource { id: id.clone() });
            }
        }

        for (id, &next_version) in next_map {
            match previous_map.get(id) {
                Some(&prev_version) if next_version != prev_version => {
                    advisory.updates.push(NewSource {
                        id: id.clone(),
                        source: initializer(id.clone()).await,
                    });
                }
                None => {
                    advisory.additions.push(NewSource {
                        id: id.clone(),
                        source: initializer(id.clone()).await,
                    });
                }
                _ => {} // Unchanged
            }
        }

        advisory
    }

    /// Returns `true` if there are no changes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.additions.is_empty() && self.updates.is_empty() && self.removals.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use insta::assert_yaml_snapshot;
    use martin_core::CacheZoomRange;
    use martin_core::tiles::{BoxedSource, MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::{TileJSON, tilejson};

    use super::*;

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

    async fn make_source(id: String) -> MartinResult<BoxedSource> {
        Ok(Box::new(TestSource {
            id,
            tj: tilejson! {
                tilejson: "3.0.0".to_string(),
                tiles: vec!["https://maplibre.org".to_string()],
                attribution: String::new(),
                name: "test_json".to_string(),
                scheme: "xyz".to_string(),
            },
        }))
    }

    #[derive(serde::Serialize)]
    struct AdvisorySnapshot {
        additions: Vec<String>,
        updates: Vec<String>,
        removals: Vec<String>,
    }

    impl From<&ReloadAdvisory> for AdvisorySnapshot {
        fn from(advisory: &ReloadAdvisory) -> Self {
            Self {
                additions: advisory.additions.iter().map(|s| s.id.clone()).collect(),
                updates: advisory.updates.iter().map(|s| s.id.clone()).collect(),
                removals: advisory.removals.iter().map(|s| s.id.clone()).collect(),
            }
        }
    }

    #[tokio::test]
    async fn from_sets_empty_to_empty() {
        let prev = BTreeSet::new();
        let next = BTreeSet::new();
        let advisory = ReloadAdvisory::from_sets(&prev, &next, make_source).await;
        assert_yaml_snapshot!(AdvisorySnapshot::from(&advisory), @"
        additions: []
        updates: []
        removals: []
        ");
    }

    #[tokio::test]
    async fn from_sets_additions_only() {
        let prev = BTreeSet::new();
        let next: BTreeSet<String> = ["a", "b"].into_iter().map(String::from).collect();
        let advisory = ReloadAdvisory::from_sets(&prev, &next, make_source).await;
        assert_yaml_snapshot!(AdvisorySnapshot::from(&advisory), @"
        additions:
          - a
          - b
        updates: []
        removals: []
        ");
    }

    #[tokio::test]
    async fn from_sets_removals_only() {
        let prev: BTreeSet<String> = ["a", "b"].into_iter().map(String::from).collect();
        let next = BTreeSet::new();
        let advisory = ReloadAdvisory::from_sets(&prev, &next, make_source).await;
        assert_yaml_snapshot!(AdvisorySnapshot::from(&advisory), @"
        additions: []
        updates: []
        removals:
          - a
          - b
        ");
    }

    #[tokio::test]
    async fn from_sets_mixed() {
        let prev: BTreeSet<String> = ["a", "b", "c"].into_iter().map(String::from).collect();
        let next: BTreeSet<String> = ["b", "c", "d"].into_iter().map(String::from).collect();
        let advisory = ReloadAdvisory::from_sets(&prev, &next, make_source).await;
        assert_yaml_snapshot!(AdvisorySnapshot::from(&advisory), @"
        additions:
          - d
        updates: []
        removals:
          - a
        ");
    }

    #[tokio::test]
    async fn from_maps_additions_and_removals() {
        let prev: BTreeMap<String, u64> = [("a".into(), 1), ("b".into(), 2)].into_iter().collect();
        let next: BTreeMap<String, u64> = [("b".into(), 2), ("c".into(), 3)].into_iter().collect();
        let advisory = ReloadAdvisory::from_maps(&prev, &next, make_source).await;
        assert_yaml_snapshot!(AdvisorySnapshot::from(&advisory), @"
        additions:
          - c
        updates: []
        removals:
          - a
        ");
    }

    #[tokio::test]
    async fn from_maps_version_update() {
        let prev: BTreeMap<String, u64> = [("a".into(), 1), ("b".into(), 2)].into_iter().collect();
        let next: BTreeMap<String, u64> = [("a".into(), 1), ("b".into(), 5)].into_iter().collect();
        let advisory = ReloadAdvisory::from_maps(&prev, &next, make_source).await;
        assert_yaml_snapshot!(AdvisorySnapshot::from(&advisory), @"
        additions: []
        updates:
          - b
        removals: []
        ");
    }

    #[tokio::test]
    async fn from_maps_all_changes() {
        let prev: BTreeMap<String, u64> = [("a".into(), 1), ("b".into(), 2), ("c".into(), 3)]
            .into_iter()
            .collect();
        let next: BTreeMap<String, u64> = [("b".into(), 9), ("c".into(), 3), ("d".into(), 4)]
            .into_iter()
            .collect();
        let advisory = ReloadAdvisory::from_maps(&prev, &next, make_source).await;
        assert_yaml_snapshot!(AdvisorySnapshot::from(&advisory), @"
        additions:
          - d
        updates:
          - b
        removals:
          - a
        ");
    }
}
