//! The generic [`ReloadDriver`] reconcile loop.

use std::collections::BTreeMap;
use std::sync::Arc;

use martin_core::tiles::BoxedSource;
use tokio::task::JoinHandle;

use crate::config::file::tiles::discovery::{Discovery, Version};
use crate::config::file::tiles::driver::{Sink, Trigger};
use crate::reload::ReloadAdvisory;
use crate::{MartinError, MartinResult};

/// What the catalog already holds for a driver's sources when it starts.
///
/// The baseline must match the catalog, since each reconcile applies the diff between it and the
/// next discovery.
#[derive(Clone, Copy)]
pub enum Baseline {
    /// Loaded into the catalog at startup by `config.resolve()` (local directories): seed the
    /// baseline from the current discovery, so only later changes apply and removals diff correctly.
    StartupResolved,
    /// Not populated yet (remote prefixes are listed only by polling): start empty, so the first
    /// reconcile loads everything discovered.
    Empty,
}

/// Establishes a [`Baseline`], then on each [`Trigger`] re-discovers, diffs, applies, and
/// commits-on-success / retains-on-failure.
pub struct ReloadDriver<D: Discovery, S: Sink> {
    /// `Arc` so a clone can move into the build closure without borrowing `self`
    /// across the spawned task's awaits.
    discovery: Arc<D>,
    sink: S,
    baseline: Option<BTreeMap<String, (Version, D::Args)>>,
}

impl<D: Discovery, S: Sink> ReloadDriver<D, S> {
    #[must_use]
    pub fn new(discovery: D, sink: S) -> Self {
        Self {
            discovery: Arc::new(discovery),
            sink,
            baseline: None,
        }
    }

    /// Establishes the [`Baseline`], then reconciles once per `trigger.next()`.
    pub fn spawn(mut self, mut trigger: impl Trigger, initial: Baseline) -> JoinHandle<()> {
        tokio::spawn(async move {
            match initial {
                Baseline::StartupResolved => self.seed().await,
                Baseline::Empty => self.baseline = Some(BTreeMap::new()),
            }
            while trigger.next().await.is_some() {
                self.reconcile().await;
            }
        })
    }

    /// Records the startup state without applying; the catalog was already populated at
    /// startup, so applying would double-add.
    async fn seed(&mut self) {
        match self.discovery.discover().await {
            Ok(next) => self.baseline = Some(next),
            Err(error) => {
                tracing::warn!(?error, "reload seed discovery failed; baseline deferred");
            }
        }
    }

    async fn reconcile(&mut self) {
        let next = match self.discovery.discover().await {
            Ok(next) => next,
            Err(error) => {
                tracing::warn!(?error, "reload discovery failed; retaining baseline");
                return;
            }
        };

        let Some(prev) = self.baseline.as_ref() else {
            // No baseline yet (the seed failed): record it without applying, so already-served
            // sources aren't re-added in a flood.
            self.baseline = Some(next);
            return;
        };

        let prev_versions: BTreeMap<String, Version> =
            prev.iter().map(|(id, (v, _))| (id.clone(), *v)).collect();
        let next_versions: BTreeMap<String, Version> =
            next.iter().map(|(id, (v, _))| (id.clone(), *v)).collect();

        let process = self.discovery.process();
        let discovery = Arc::clone(&self.discovery);
        let args_by_id: BTreeMap<String, D::Args> = next
            .iter()
            .map(|(id, (_, args))| (id.clone(), args.clone()))
            .collect();

        let advisory = ReloadAdvisory::from_maps(
            &prev_versions,
            &next_versions,
            async move |id: String| -> MartinResult<BoxedSource> {
                let args = args_by_id
                    .get(&id)
                    .ok_or_else(|| MartinError::SourceNotFound(id.clone()))?;
                discovery.build(&id, args).await
            },
            process,
        )
        .await;

        match self.sink.apply_changes(advisory).await {
            Ok(()) => self.baseline = Some(next),
            Err(error) => {
                tracing::warn!(?error, "reload apply failed; retaining baseline for retry");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use rstest::rstest;

    use martin_core::CacheZoomRange;
    use martin_core::tiles::{MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::{TileJSON, tilejson};

    use super::*;
    use crate::config::file::ProcessConfig;

    /// A minimal in-memory [`Source`] returning a fixed tile; used to populate advisories.
    #[derive(Debug, Clone)]
    struct TestSource {
        id: String,
        tj: TileJSON,
    }

    impl TestSource {
        fn new(id: impl Into<String>) -> Self {
            Self {
                id: id.into(),
                tj: tilejson! { tiles: vec!["https://example.com".to_string()] },
            }
        }
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

    /// Projects a [`ReloadAdvisory`] to the source ids in each bucket, for order-sensitive equality.
    #[derive(Debug, Clone, PartialEq, Eq)]
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

    type Snapshot = BTreeMap<String, (Version, ())>;

    fn snapshot(entries: &[(&str, Version)]) -> Snapshot {
        entries
            .iter()
            .map(|(id, v)| ((*id).to_string(), (*v, ())))
            .collect()
    }

    /// A snapshot of `Opaque` (unversioned) sources, as the remote object-store path produces.
    fn snapshot_opaque(ids: &[&str]) -> Snapshot {
        snapshot(
            &ids.iter()
                .map(|id| (*id, Version::Opaque))
                .collect::<Vec<_>>(),
        )
    }

    /// Replays a scripted sequence of `discover()` results.
    struct FakeDiscovery {
        snapshots: Mutex<VecDeque<MartinResult<Snapshot>>>,
    }

    impl FakeDiscovery {
        fn new(snapshots: Vec<MartinResult<Snapshot>>) -> Self {
            Self {
                snapshots: Mutex::new(snapshots.into()),
            }
        }
    }

    impl Discovery for FakeDiscovery {
        type Args = ();

        async fn discover(&self) -> MartinResult<Snapshot> {
            self.snapshots
                .lock()
                .expect("FakeDiscovery mutex poisoned")
                .pop_front()
                .unwrap_or_else(|| Ok(Snapshot::new()))
        }

        async fn build(&self, id: &str, _args: &()) -> MartinResult<BoxedSource> {
            Ok(Box::new(TestSource::new(id)))
        }

        fn process(&self) -> ProcessConfig {
            ProcessConfig::default()
        }
    }

    /// Fires `remaining` times, then signals shutdown.
    struct ManualTrigger {
        remaining: usize,
    }

    impl ManualTrigger {
        fn new(ticks: usize) -> Self {
            Self { remaining: ticks }
        }
    }

    impl Trigger for ManualTrigger {
        async fn next(&mut self) -> Option<()> {
            if self.remaining == 0 {
                return None;
            }
            self.remaining -= 1;
            Some(())
        }
    }

    /// Records every applied advisory and replays scripted results.
    #[derive(Clone)]
    struct SpySink {
        applied: Arc<Mutex<Vec<AdvisorySnapshot>>>,
        results: Arc<Mutex<VecDeque<MartinResult<()>>>>,
    }

    impl SpySink {
        fn new() -> Self {
            Self {
                applied: Arc::new(Mutex::new(Vec::new())),
                results: Arc::new(Mutex::new(VecDeque::new())),
            }
        }

        fn with_results(results: Vec<MartinResult<()>>) -> Self {
            let s = Self::new();
            *s.results.lock().expect("SpySink results mutex poisoned") = results.into();
            s
        }

        fn recorded(&self) -> Arc<Mutex<Vec<AdvisorySnapshot>>> {
            Arc::clone(&self.applied)
        }
    }

    impl Sink for SpySink {
        async fn apply_changes(&self, advisory: ReloadAdvisory) -> MartinResult<()> {
            self.applied
                .lock()
                .expect("SpySink applied mutex poisoned")
                .push(AdvisorySnapshot::from(&advisory));
            self.results
                .lock()
                .expect("SpySink results mutex poisoned")
                .pop_front()
                .unwrap_or(Ok(()))
        }
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_string()).collect()
    }

    /// One tick diffs the seeded baseline against the next discovery and applies a single advisory.
    /// The `opaque_unchanged` case pins the `Version::Opaque` contract: two equal `Opaque` versions never update.
    #[rstest]
    #[case::addition(
        snapshot(&[]),
        snapshot(&[("a", Version::Tracked(1))]),
        ids(&["a"]), ids(&[]), ids(&[]),
    )]
    #[case::update(
        snapshot(&[("a", Version::Tracked(1))]),
        snapshot(&[("a", Version::Tracked(2))]),
        ids(&[]), ids(&["a"]), ids(&[]),
    )]
    #[case::removal(
        snapshot(&[("a", Version::Tracked(1))]),
        snapshot(&[]),
        ids(&[]), ids(&[]), ids(&["a"]),
    )]
    #[case::opaque_unchanged(
        snapshot_opaque(&["a"]),
        snapshot_opaque(&["a"]),
        ids(&[]), ids(&[]), ids(&[]),
    )]
    #[tokio::test]
    async fn tick_diffs_baseline_against_discovery(
        #[case] before: Snapshot,
        #[case] after: Snapshot,
        #[case] additions: Vec<String>,
        #[case] updates: Vec<String>,
        #[case] removals: Vec<String>,
    ) {
        let discovery = FakeDiscovery::new(vec![Ok(before), Ok(after)]);
        let sink = SpySink::new();
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(1), Baseline::StartupResolved)
            .await
            .expect("driver task panicked");

        assert_eq!(
            *recorded.lock().unwrap(),
            vec![AdvisorySnapshot {
                additions,
                updates,
                removals,
            }]
        );
    }

    #[tokio::test]
    async fn unseeded_applies_full_first_discovery() {
        // The remote poll path: nothing pre-populated the catalog, so the first tick must apply
        // the entire discovery rather than recording it as an already-applied baseline.
        let discovery = FakeDiscovery::new(vec![Ok(snapshot(&[
            ("a", Version::Tracked(1)),
            ("b", Version::Tracked(1)),
        ]))]);
        let sink = SpySink::new();
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(1), Baseline::Empty)
            .await
            .expect("driver task panicked");

        assert_eq!(
            *recorded.lock().unwrap(),
            vec![AdvisorySnapshot {
                additions: ids(&["a", "b"]),
                updates: ids(&[]),
                removals: ids(&[]),
            }]
        );
    }

    #[tokio::test]
    async fn seed_does_not_apply() {
        // No triggers: the driver only seeds, which must not apply (catalog already populated).
        let discovery = FakeDiscovery::new(vec![Ok(snapshot(&[("a", Version::Tracked(1))]))]);
        let sink = SpySink::new();
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(0), Baseline::StartupResolved)
            .await
            .expect("driver task panicked");

        assert!(recorded.lock().unwrap().is_empty(), "seed must not apply");
    }

    #[tokio::test]
    async fn failed_seed_then_success_does_not_flood() {
        // Seed fails (baseline stays None); the first good tick establishes it without applying.
        let discovery = FakeDiscovery::new(vec![
            Err(MartinError::SourceNotFound("seed boom".into())),
            Ok(snapshot(&[
                ("a", Version::Tracked(1)),
                ("b", Version::Tracked(1)),
            ])),
        ]);
        let sink = SpySink::new();
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(1), Baseline::StartupResolved)
            .await
            .expect("driver task panicked");

        assert!(
            recorded.lock().unwrap().is_empty(),
            "establishing the baseline after a failed seed must not flood"
        );
    }

    #[tokio::test]
    async fn failed_discover_retains_baseline() {
        // The failed middle tick keeps the baseline, so only `b` diffs on the last tick.
        let discovery = FakeDiscovery::new(vec![
            Ok(snapshot(&[("a", Version::Tracked(1))])),
            Err(MartinError::SourceNotFound("tick boom".into())),
            Ok(snapshot(&[
                ("a", Version::Tracked(1)),
                ("b", Version::Tracked(1)),
            ])),
        ]);
        let sink = SpySink::new();
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(2), Baseline::StartupResolved)
            .await
            .expect("driver task panicked");

        assert_eq!(
            *recorded.lock().unwrap(),
            vec![AdvisorySnapshot {
                additions: ids(&["b"]),
                updates: ids(&[]),
                removals: ids(&[]),
            }]
        );
    }

    #[tokio::test]
    async fn failed_apply_retains_baseline_and_retries() {
        // The first apply fails; the retained baseline makes the next tick retry the same delta.
        let discovery = FakeDiscovery::new(vec![
            Ok(snapshot(&[])),
            Ok(snapshot(&[("a", Version::Tracked(1))])),
            Ok(snapshot(&[("a", Version::Tracked(1))])),
        ]);
        let sink = SpySink::with_results(vec![
            Err(MartinError::SourceNotFound("apply boom".into())),
            Ok(()),
        ]);
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(2), Baseline::StartupResolved)
            .await
            .expect("driver task panicked");

        let add_a = AdvisorySnapshot {
            additions: ids(&["a"]),
            updates: ids(&[]),
            removals: ids(&[]),
        };
        assert_eq!(*recorded.lock().unwrap(), vec![add_a.clone(), add_a]);
    }
}
