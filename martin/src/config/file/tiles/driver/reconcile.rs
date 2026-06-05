//! The generic [`ReloadDriver`] reconcile loop.

use std::collections::BTreeMap;
use std::sync::Arc;

use martin_core::tiles::BoxedSource;
use tokio::task::JoinHandle;

use crate::config::file::tiles::discovery::{Discovery, Version};
use crate::config::file::tiles::driver::{Sink, Trigger};
use crate::reload::ReloadAdvisory;
use crate::{MartinError, MartinResult};

pub struct ReloadDriver<D: Discovery, S: Sink> {
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

    pub fn spawn(mut self, mut trigger: impl Trigger) -> JoinHandle<()> {
        tokio::spawn(async move {
            self.seed().await;
            while trigger.next().await.is_some() {
                self.reconcile().await;
            }
        })
    }

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

    use martin_core::CacheZoomRange;
    use martin_core::tiles::{MartinCoreResult, Source, UrlQuery};
    use martin_tile_utils::{Encoding, Format, TileCoord, TileData, TileInfo};
    use tilejson::{TileJSON, tilejson};

    use super::*;
    use crate::config::file::ProcessConfig;

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

    fn snapshot(entries: &[(&str, u128)]) -> Snapshot {
        entries
            .iter()
            .map(|(id, v)| ((*id).to_string(), (*v, ())))
            .collect()
    }

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

    #[tokio::test]
    async fn seed_does_not_apply() {
        let discovery = FakeDiscovery::new(vec![Ok(snapshot(&[("a", 1)]))]);
        let sink = SpySink::new();
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(0))
            .await
            .expect("driver task panicked");

        assert!(recorded.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_seed_then_success_does_not_flood() {
        let discovery = FakeDiscovery::new(vec![
            Err(MartinError::SourceNotFound("seed boom".into())),
            Ok(snapshot(&[("a", 1), ("b", 1)])),
        ]);
        let sink = SpySink::new();
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(1))
            .await
            .expect("driver task panicked");

        assert!(recorded.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_discover_retains_baseline() {
        let discovery = FakeDiscovery::new(vec![
            Ok(snapshot(&[("a", 1)])),
            Err(MartinError::SourceNotFound("tick boom".into())),
            Ok(snapshot(&[("a", 1), ("b", 1)])),
        ]);
        let sink = SpySink::new();
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(2))
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
        let discovery = FakeDiscovery::new(vec![
            Ok(snapshot(&[])),
            Ok(snapshot(&[("a", 1)])),
            Ok(snapshot(&[("a", 1)])),
        ]);
        let sink = SpySink::with_results(vec![
            Err(MartinError::SourceNotFound("apply boom".into())),
            Ok(()),
        ]);
        let recorded = sink.recorded();

        ReloadDriver::new(discovery, sink)
            .spawn(ManualTrigger::new(2))
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
