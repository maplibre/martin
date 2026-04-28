use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use futures::future::try_join_all;
use futures::stream::TryStreamExt as _;
use object_store::ObjectStore as _;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::time::{Instant, MissedTickBehavior};
use url::Url;

use crate::config::file::{
    CachePolicy, ConfigFileError, FileConfigEnum, FileConfigSrc, TileSourceConfiguration as _,
    tiles::pmtiles::PmtConfig,
};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::reload::ReloadAdvisory;
use crate::tile_source_manager::TileSourceManager;
use crate::{MartinError, MartinResult};

const PMTILES_EXTENSION: &str = ".pmtiles";

/// A periodic, polling-based reloader for `PMTiles` sources.
///
/// On each tick (default every 600 s; configured via `PmtConfig::reload_interval_secs`) it
/// lists every directory in `pmtiles.paths` via `object_store` (works uniformly for local
/// dirs and remote prefixes such as `s3://bucket/`), then diffs the discovered ids against
/// the previous tick to issue **add** and **remove** advisories via
/// [`ReloadAdvisory::from_sets`] / [`TileSourceManager::apply_changes`].
///
/// **Updates are not detected by polling.** Each `PmtilesSource` carries a reload-signal
/// channel back to this reloader. When `pmtiles` 0.23 detects that the underlying blob's
/// `data_version_string` (`ETag`) changed at tile-fetch time (`PmtError::SourceModified`),
/// the source kicks the channel and the reloader rebuilds that single source immediately
/// — no waiting for the next tick, no per-source HEAD calls.
pub struct PMTilesReloader {
    id_resolver: IdResolver,
    tile_source_manager: TileSourceManager,
    pmt_config: PmtConfig,
    /// Local or URL-shaped prefixes to list each tick.
    prefixes: Vec<PathBuf>,
    /// Individually-configured sources from `pmtiles.sources`. Carried through so the
    /// listing-driven diff doesn't accidentally remove them; their updates are handled via
    /// the signal channel.
    tracked_singles: BTreeMap<String, FileConfigSrc>,
    /// Last-known `id -> (URL, cache policy)` map. Used both to diff against the next
    /// listing and to look up the URL when rebuilding a single source on signal.
    sources: BTreeMap<String, TrackedSource>,
    /// Receiver paired with the sender installed on each `PmtilesSource`. `take`n into the
    /// spawned task on `start`.
    signal_rx: Option<UnboundedReceiver<String>>,
}

#[derive(Clone, Debug)]
struct TrackedSource {
    url: Url,
    cache: CachePolicy,
}

impl PMTilesReloader {
    /// Builds a reloader from the post-`finalize` / post-`resolve` `PMTiles` config.
    ///
    /// Snapshots `pmtiles.paths` (local directories or URL-shaped prefixes such as
    /// `s3://bucket/`) and `pmtiles.sources`. `signal_rx` is the receiving end of the
    /// channel whose sender was installed on `PmtConfig::reload_signal` before
    /// `Config::resolve` ran.
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<PmtConfig>,
        signal_rx: UnboundedReceiver<String>,
    ) -> Self {
        let mut prefixes: Vec<PathBuf> = vec![];
        let mut tracked_singles: BTreeMap<String, FileConfigSrc> = BTreeMap::new();
        let mut pmt_config = PmtConfig::default();

        match config {
            FileConfigEnum::Config(cfg) => {
                pmt_config = cfg.custom.clone();
                match &cfg.paths {
                    OptOneMany::One(p) => prefixes.push(p.clone()),
                    OptOneMany::Many(ps) => prefixes.extend(ps.iter().cloned()),
                    OptOneMany::NoVals => {}
                }
                if let Some(srcs) = &cfg.sources {
                    for (id, src) in srcs {
                        tracked_singles.insert(id.clone(), src.clone());
                    }
                }
            }
            // After `resolve_files`, an empty-but-watched directory collapses back to
            // `Path` / `Paths` (no sources, one or more paths) rather than `Config`.
            FileConfigEnum::Path(p) => prefixes.push(p.clone()),
            FileConfigEnum::Paths(ps) => prefixes.extend(ps.iter().cloned()),
            FileConfigEnum::None => {}
        }

        prefixes.sort();
        prefixes.dedup();

        Self {
            tile_source_manager: tsm,
            id_resolver,
            pmt_config,
            prefixes,
            tracked_singles,
            sources: BTreeMap::new(),
            signal_rx: Some(signal_rx),
        }
    }

    fn interval(&self) -> Duration {
        Duration::from_secs(self.pmt_config.reload_interval_secs)
    }

    /// Spawns the polling + signal loop. Returns immediately.
    ///
    /// No-op if the reload interval is zero or there is nothing to watch.
    pub fn start(mut self) {
        let interval = self.interval();
        if interval.is_zero() {
            tracing::info!("PMTilesReloader disabled (reload_interval_secs = 0)");
            return;
        }
        if self.prefixes.is_empty() && self.tracked_singles.is_empty() {
            return;
        }

        let mut tsm = self.tile_source_manager.clone();
        let mut signal_rx = self
            .signal_rx
            .take()
            .expect("signal_rx is taken once on start");

        tokio::spawn(async move {
            match self.discover_sources().await {
                Ok(initial) => self.sources = initial,
                Err(e) => tracing::warn!("initial pmtiles discovery failed: {e:?}"),
            }

            let start = Instant::now() + interval;
            let mut ticker = tokio::time::interval_at(start, interval);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

            loop {
                tokio::select! {
                    _ = ticker.tick() => self.tick(&mut tsm).await,
                    Some(id) = signal_rx.recv() => self.reload_one(&id, &mut tsm).await,
                }
            }
        });
    }

    /// Periodic listing pass: discovers any current set of `id -> TrackedSource` and applies
    /// add/remove diffs via `ReloadAdvisory::from_sets`.
    async fn tick(&mut self, tsm: &mut TileSourceManager) {
        let next = match self.discover_sources().await {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("PMTilesReloader: discovery failed: {e:?}");
                return;
            }
        };

        let prev_ids: BTreeSet<String> = self.sources.keys().cloned().collect();
        let next_ids: BTreeSet<String> = next.keys().cloned().collect();
        let next_clone = next.clone();
        let pmt_config = self.pmt_config.clone();

        let adv = ReloadAdvisory::from_sets(&prev_ids, &next_ids, async move |id| {
            let entry = next_clone.get(&id).ok_or_else(|| {
                MartinError::from(ConfigFileError::InvalidSourceFilePath(
                    id.clone(),
                    PathBuf::new(),
                ))
            })?;
            pmt_config
                .new_sources_url(id, entry.url.clone(), entry.cache)
                .await
        })
        .await;

        match tsm.apply_changes(adv).await {
            Ok(()) => self.sources = next,
            Err(e) => tracing::warn!("PMTilesReloader: apply_changes failed: {e:?}"),
        }
    }

    /// Signal-driven single-source rebuild: re-create the `BoxedSource` for `id` and apply
    /// it as an `update` so the `TileSourceManager` invalidates the tile cache and swaps
    /// the source atomically.
    async fn reload_one(&mut self, id: &str, tsm: &mut TileSourceManager) {
        let Some(entry) = self.sources.get(id).cloned() else {
            tracing::warn!("PMTilesReloader: signal received for unknown source {id}");
            return;
        };
        let new_source = self
            .pmt_config
            .new_sources_url(id.to_string(), entry.url.clone(), entry.cache)
            .await;

        let adv = ReloadAdvisory {
            updates: vec![crate::reload::NewSource {
                id: id.to_string(),
                source: new_source,
            }],
            ..Default::default()
        };
        if let Err(e) = tsm.apply_changes(adv).await {
            tracing::warn!("PMTilesReloader: signal-driven reload of {id} failed: {e:?}");
        }
    }

    /// Lists every prefix in parallel and merges the results with the always-present
    /// `tracked_singles`. Failures of individual prefix lists are logged and treated as
    /// "this prefix contributed nothing this tick" (so a transient remote outage doesn't
    /// flap the catalog).
    async fn discover_sources(&self) -> MartinResult<BTreeMap<String, TrackedSource>> {
        let prefix_results =
            try_join_all(self.prefixes.iter().map(|p| self.list_prefix(p))).await?;

        let mut out: BTreeMap<String, TrackedSource> = BTreeMap::new();
        let mut seen_urls: BTreeSet<String> = BTreeSet::new();
        for entries in prefix_results {
            for (id, ts) in entries {
                seen_urls.insert(ts.url.to_string());
                out.insert(id, ts);
            }
        }

        // Carry tracked singles through unchanged: their URL came from the user, they exist
        // unconditionally, and updates to them flow through the signal channel.
        for (id, src) in &self.tracked_singles {
            let path = src.get_path();
            let Some(url) = path
                .to_str()
                .and_then(|s| Url::parse(s).ok())
                .or_else(|| Url::from_file_path(path).ok())
            else {
                tracing::warn!("cannot resolve URL for source {id} ({path:?})");
                continue;
            };
            if seen_urls.contains(url.as_str()) {
                continue;
            }
            out.insert(
                id.clone(),
                TrackedSource {
                    url,
                    cache: src.cache_zoom(),
                },
            );
        }

        Ok(out)
    }

    async fn list_prefix(
        &self,
        prefix_path: &PathBuf,
    ) -> MartinResult<Vec<(String, TrackedSource)>> {
        // Accept either a URL-shaped string (s3://bucket/path/) or an absolute local path.
        let Some(url) = prefix_path
            .to_str()
            .and_then(|s| Url::parse(s).ok())
            .or_else(|| Url::from_file_path(prefix_path).ok())
        else {
            tracing::warn!(
                "prefix {prefix_path:?} is neither a URL nor an absolute path; skipping"
            );
            return Ok(Vec::new());
        };
        let (store, base) = object_store::parse_url_opts(&url, &self.pmt_config.options)
            .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, url.to_string()))?;

        let mut out = Vec::new();
        let mut stream = store.list(Some(&base));
        loop {
            let meta = match stream.try_next().await {
                Ok(Some(m)) => m,
                Ok(None) => break,
                Err(e) => {
                    tracing::warn!("list failed for {url}: {e}; skipping prefix this tick");
                    break;
                }
            };
            if !meta.location.as_ref().ends_with(PMTILES_EXTENSION) {
                continue;
            }
            let stem = meta
                .location
                .filename()
                .and_then(|f| f.strip_suffix(PMTILES_EXTENSION))
                .unwrap_or("_unknown");
            // `meta.location` is reported relative to the *store's* root (which is `/` for
            // the local backend and the bucket for s3/gs/azure), not relative to the prefix
            // we asked to list. Reconstruct the absolute URL from scheme + authority +
            // location so it round-trips through `new_sources_url`.
            let object_url_str = format!(
                "{}://{}/{}",
                url.scheme(),
                url.host_str().unwrap_or(""),
                meta.location
            );
            let Ok(object_url) = Url::parse(&object_url_str) else {
                tracing::warn!("cannot build absolute URL from {object_url_str}");
                continue;
            };
            let id = self.id_resolver.resolve(stem, object_url.to_string());
            out.push((
                id,
                TrackedSource {
                    url: object_url,
                    cache: CachePolicy::default(),
                },
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use insta::assert_yaml_snapshot;
    use tokio::sync::mpsc::unbounded_channel;

    use super::*;
    use crate::config::file::tiles::pmtiles::DEFAULT_RELOAD_INTERVAL_SECS;
    use crate::config::file::{FileConfig, FileConfigSource, OnInvalid};
    use crate::config::primitives::OptOneMany;

    fn make_reloader(config: &FileConfigEnum<PmtConfig>) -> PMTilesReloader {
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);
        let resolver = IdResolver::new(&[]);
        let (_tx, rx) = unbounded_channel();
        PMTilesReloader::new(tsm, resolver, config, rx)
    }

    #[test]
    fn new_with_none_config_yields_default_interval() {
        let reloader = make_reloader(&FileConfigEnum::None);
        assert!(reloader.prefixes.is_empty());
        assert!(reloader.tracked_singles.is_empty());
        assert_eq!(
            reloader.interval(),
            Duration::from_secs(DEFAULT_RELOAD_INTERVAL_SECS)
        );
    }

    #[derive(serde::Serialize)]
    struct ReloaderSnapshot {
        prefix_count: usize,
        single_ids: Vec<String>,
        interval_secs: u64,
    }

    impl From<&PMTilesReloader> for ReloaderSnapshot {
        fn from(r: &PMTilesReloader) -> Self {
            Self {
                prefix_count: r.prefixes.len(),
                single_ids: r.tracked_singles.keys().cloned().collect(),
                interval_secs: r.interval().as_secs(),
            }
        }
    }

    #[test]
    fn new_extracts_paths_and_sources() {
        let mut sources: BTreeMap<String, FileConfigSrc> = BTreeMap::new();
        sources.insert(
            "src_a".to_string(),
            FileConfigSrc::Obj(FileConfigSource {
                path: PathBuf::from("/tmp/a.pmtiles"),
                cache: CachePolicy::default(),
            }),
        );
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![PathBuf::from("/tmp/dir1"), PathBuf::from("/tmp/dir2")]),
            sources: Some(sources),
            custom: PmtConfig {
                reload_interval_secs: 30,
                ..PmtConfig::default()
            },
        });

        assert_yaml_snapshot!(ReloaderSnapshot::from(&make_reloader(&cfg)), @r"
        prefix_count: 2
        single_ids:
          - src_a
        interval_secs: 30
        ");
    }

    #[test]
    fn new_dedups_prefixes() {
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![PathBuf::from("/tmp/dir"), PathBuf::from("/tmp/dir")]),
            sources: None,
            custom: PmtConfig::default(),
        });
        assert_yaml_snapshot!(ReloaderSnapshot::from(&make_reloader(&cfg)), @r"
        prefix_count: 1
        single_ids: []
        interval_secs: 600
        ");
    }
}

/// Integration tests using a `MinIO` testcontainer alongside a local-file prefix to exercise
/// `discover_sources` end-to-end across both backends. Requires Docker.
#[cfg(test)]
mod minio_tests {
    use std::collections::HashMap;

    use insta::assert_yaml_snapshot;
    use object_store::ObjectStoreExt as _;
    use object_store::PutPayload;
    use object_store::path::Path as ObjPath;
    use tempfile::tempdir;
    use testcontainers_modules::minio::MinIO;
    use testcontainers_modules::testcontainers::core::{CmdWaitFor, ExecCommand};
    use testcontainers_modules::testcontainers::runners::AsyncRunner as _;
    use tokio::sync::mpsc::unbounded_channel;

    use super::*;
    use crate::config::file::{FileConfig, OnInvalid};

    const BUCKET: &str = "pmt-bucket";
    /// A small valid `.pmtiles` blob (47 KB) used for upload payloads.
    const FIXTURE_A: &[u8] =
        include_bytes!("../../../../../../tests/fixtures/pmtiles2/webp2.pmtiles");

    fn s3_options(endpoint: &str) -> HashMap<String, String> {
        let mut o = HashMap::new();
        o.insert("aws_endpoint".into(), endpoint.to_string());
        o.insert("aws_access_key_id".into(), "minioadmin".into());
        o.insert("aws_secret_access_key".into(), "minioadmin".into());
        o.insert("aws_region".into(), "us-east-1".into());
        o.insert("allow_http".into(), "true".into());
        o.insert("virtual_hosted_style_request".into(), "false".into());
        o
    }

    fn sorted_ids(map: &BTreeMap<String, TrackedSource>) -> Vec<String> {
        map.keys().cloned().collect()
    }

    #[tokio::test]
    async fn discovery_handles_add_and_remove_across_s3_and_local() {
        // MinIO maps subdirectories of /data to buckets, so creating the directory creates
        // the bucket without needing an mc client or signed PUT request.
        let minio = MinIO::default().start().await.unwrap();
        minio
            .exec(
                ExecCommand::new(["mkdir", &format!("/data/{BUCKET}")])
                    .with_cmd_ready_condition(CmdWaitFor::exit()),
            )
            .await
            .unwrap();

        let host = minio.get_host().await.unwrap();
        let port = minio.get_host_port_ipv4(9000).await.unwrap();
        let endpoint = format!("http://{host}:{port}");
        let options = s3_options(&endpoint);

        let s3_url: Url = format!("s3://{BUCKET}/").parse().unwrap();
        let (s3_store, _base) = object_store::parse_url_opts(&s3_url, &options).unwrap();
        s3_store
            .put(
                &ObjPath::from("a.pmtiles"),
                PutPayload::from_static(FIXTURE_A),
            )
            .await
            .unwrap();
        s3_store
            .put(
                &ObjPath::from("b.pmtiles"),
                PutPayload::from_static(FIXTURE_A),
            )
            .await
            .unwrap();

        let local_dir = tempdir().unwrap();
        std::fs::write(local_dir.path().join("local.pmtiles"), FIXTURE_A).unwrap();

        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![
                local_dir.path().to_path_buf(),
                PathBuf::from(format!("s3://{BUCKET}/")),
            ]),
            sources: None,
            custom: PmtConfig {
                reload_interval_secs: 1,
                options: options.clone(),
                ..PmtConfig::default()
            },
        });
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);
        let resolver = IdResolver::new(&[]);
        let (_tx, rx) = unbounded_channel();
        let reloader = PMTilesReloader::new(tsm, resolver, &cfg, rx);

        let initial = reloader.discover_sources().await.unwrap();
        assert_yaml_snapshot!(sorted_ids(&initial), @r"
        - a
        - b
        - local
        ");

        // Removing one s3 object and adding a new local file should reflect both.
        s3_store.delete(&ObjPath::from("b.pmtiles")).await.unwrap();
        std::fs::write(local_dir.path().join("local2.pmtiles"), FIXTURE_A).unwrap();
        let after = reloader.discover_sources().await.unwrap();
        assert_yaml_snapshot!(sorted_ids(&after), @r"
        - a
        - local
        - local2
        ");
    }
}
