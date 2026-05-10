use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use super::{discover_sources_by_ext, path_modified_ms};

use futures::stream::TryStreamExt as _;
use martin_core::tiles::BoxedSource;
use notify::{
    Config, Event, EventKind, RecommendedWatcher, Watcher as _,
    event::{AccessKind, AccessMode},
};
use object_store::ObjectStore as _;
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior};
use url::Url;

use crate::config::file::process::ProcessConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::resolve_process_config;
use crate::config::file::{
    CachePolicy, ConfigFileError, FileConfigEnum, TileSourceConfiguration as _,
    file_config::is_remote_url, pmtiles::PmtConfig,
};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::{MartinError, MartinResult, ReloadAdvisory, TileSourceManager};

const PMTILES_EXT: &str = "pmtiles";
const PMTILES_EXT_DOT: &str = ".pmtiles";

/// A reloader for `PMTiles` sources.
///
/// Local directories are watched via `notify` filesystem events (sub-second feedback).
/// Remote URL prefixes (`s3://`, `gs://`, `https://`, …) are polled every
/// [`PmtConfig::reload_interval_secs`] seconds via `object_store::list`, which is the only
/// portable way to discover blob-store changes.
pub struct PMTilesReloader {
    id_resolver: IdResolver,
    tile_source_manager: TileSourceManager,
    /// Last-known local-source state: `id -> (canonical path, modified-ms, cache policy)`.
    sources: BTreeMap<String, (PathBuf, u128, CachePolicy)>,
    /// Local directories watched via `notify`.
    directories: Vec<PathBuf>,
    /// Maps canonical paths of explicitly configured sources to their cache policy,
    /// so directory-discovered sources that match a configured path inherit its policy.
    path_cache: BTreeMap<PathBuf, CachePolicy>,
    /// Retained to create new `PmtilesSource` instances; shares the directory cache Arc.
    config: PmtConfig,
    /// Process config (MVT/MLT conversion) applied to dynamically-discovered sources.
    process: ProcessConfig,
    /// Remote URL prefixes (e.g. `s3://bucket/`, `https://host/dir/`) to poll for sources.
    remote_prefixes: Vec<Url>,
    /// Last-known map of `id -> object URL` for prefix-discovered remote sources, used
    /// to diff each polling tick.
    remote_sources: BTreeMap<String, Url>,
}

impl PMTilesReloader {
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<PmtConfig>,
        global_process: &ProcessConfig,
    ) -> Self {
        let mut sources: BTreeMap<String, (PathBuf, u128, CachePolicy)> = BTreeMap::new();
        let mut directories: Vec<PathBuf> = vec![];
        let mut path_cache: BTreeMap<PathBuf, CachePolicy> = BTreeMap::new();
        let mut remote_prefixes: Vec<Url> = vec![];

        #[cfg(all(feature = "mlt", feature = "_tiles"))]
        let process = {
            let source_type = match config {
                FileConfigEnum::Config(cfg) => ProcessConfig {
                    convert_to_mlt: cfg.custom.convert_to_mlt.clone(),
                    convert_to_mvt: cfg.custom.convert_to_mvt.clone(),
                },
                _ => ProcessConfig::default(),
            };
            resolve_process_config(global_process, &source_type, &ProcessConfig::default())
        };
        #[cfg(not(feature = "mlt"))]
        let process = {
            let _ = global_process;
            ProcessConfig::default()
        };

        if let FileConfigEnum::Config(cfg) = config
            && let Some(s) = &cfg.sources
        {
            for (id, src) in s {
                let path = src.get_path();
                if is_remote_url(path) {
                    tracing::debug!("skipping remote URL source {:?} in PMTilesReloader", path);
                    continue;
                }
                let policy = src.cache_zoom();
                let Ok(canonical) = path.canonicalize() else {
                    tracing::warn!(
                        "failed to resolve canonical path for tile source {:?}",
                        path
                    );
                    continue;
                };
                let Some(modified_ms) = path_modified_ms(path) else {
                    continue;
                };

                path_cache.insert(canonical.clone(), policy);
                sources.insert(id.clone(), (canonical.clone(), modified_ms, policy));
            }
        }

        let mut push_path = |path: &PathBuf| {
            if is_remote_url(path) {
                let Some(url) = path.to_str().and_then(|s| Url::parse(s).ok()) else {
                    tracing::warn!(
                        "remote URL prefix {:?} could not be parsed as URL; skipping",
                        path
                    );
                    return;
                };
                remote_prefixes.push(url);
                return;
            }
            match path.canonicalize() {
                Ok(p) => directories.push(p),
                Err(e) => tracing::warn!("failed to canonicalize watch directory {:?}: {e}", path),
            }
        };

        match config {
            FileConfigEnum::Config(cfg) => match &cfg.paths {
                OptOneMany::One(path) => push_path(path),
                OptOneMany::Many(paths) => paths.iter().for_each(&mut push_path),
                OptOneMany::NoVals => {}
            },
            FileConfigEnum::Path(path) => push_path(path),
            FileConfigEnum::Paths(paths) => paths.iter().for_each(push_path),
            FileConfigEnum::None => {}
        }

        directories.sort();
        directories.dedup();
        remote_prefixes.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        remote_prefixes.dedup();

        let pmt_config = if let FileConfigEnum::Config(cfg) = config {
            cfg.custom.clone()
        } else {
            PmtConfig::default()
        };

        Self {
            tile_source_manager: tsm,
            id_resolver,
            sources,
            directories,
            path_cache,
            config: pmt_config,
            process,
            remote_prefixes,
            remote_sources: BTreeMap::new(),
        }
    }

    /// Polling cadence for remote URL prefixes. Local directories ignore this.
    #[cfg(test)]
    fn remote_poll_interval(&self) -> Duration {
        Duration::from_secs(self.config.reload_interval_secs)
    }

    pub fn start(self) -> MartinResult<()> {
        if self.directories.is_empty() && self.remote_prefixes.is_empty() {
            return Ok(());
        }

        let Self {
            id_resolver,
            tile_source_manager,
            sources,
            directories,
            path_cache,
            config,
            process,
            remote_prefixes,
            remote_sources,
        } = self;
        let interval = Duration::from_secs(config.reload_interval_secs);

        // Local directories: notify-driven event loop. Each event triggers a fresh
        // directory scan + diff. The watcher and its mutable state are owned entirely by
        // the spawned task — no shared mutex with the remote polling task.
        if !directories.is_empty() {
            let (tx, mut rx) = mpsc::channel::<Event>(256);
            let mut watcher = RecommendedWatcher::new(
                move |result: notify::Result<Event>| {
                    if let Ok(event) = result {
                        let _ = tx.blocking_send(event);
                    }
                },
                Config::default(),
            )
            .map_err(|e| MartinError::DirectoryWatchError(e.kind))?;
            for dir in &directories {
                watcher
                    // FIXME: find a naming scheme for paths that makes sense under recursive and enable it
                    .watch(dir, notify::RecursiveMode::NonRecursive)
                    .map_err(|e| MartinError::DirectoryWatchError(e.kind))?;
            }

            let mut local_state = LocalState {
                id_resolver: id_resolver.clone(),
                sources,
                directories,
                path_cache,
                config: config.clone(),
                process: process.clone(),
            };
            let mut tsm_local = tile_source_manager.clone();
            tokio::spawn(async move {
                let _watcher = watcher;
                while let Some(event) = rx.recv().await {
                    local_state.process_event(&mut tsm_local, event).await;
                }
            });
        }

        // Remote URL prefixes: polling loop. Owns its own state, ticks every
        // `reload_interval_secs` (or never if 0).
        if !remote_prefixes.is_empty() {
            if interval.is_zero() {
                tracing::info!(
                    "PMTilesReloader: remote prefix polling disabled (reload_interval_secs = 0)"
                );
            } else {
                let mut remote_state = RemoteState {
                    id_resolver,
                    remote_prefixes,
                    remote_sources,
                    config,
                    process,
                };
                let mut tsm_remote = tile_source_manager;
                tokio::spawn(async move {
                    // Tick immediately on startup so remote sources show up without waiting
                    // a full interval.
                    let mut ticker = tokio::time::interval_at(Instant::now(), interval);
                    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
                    loop {
                        ticker.tick().await;
                        remote_state.tick(&mut tsm_remote).await;
                    }
                });
            }
        }

        Ok(())
    }
}

/// Mutable state owned by the local-directory watcher task. Held entirely inside the spawned
/// task — no shared mutex with the remote-polling task.
struct LocalState {
    id_resolver: IdResolver,
    sources: BTreeMap<String, (PathBuf, u128, CachePolicy)>,
    directories: Vec<PathBuf>,
    path_cache: BTreeMap<PathBuf, CachePolicy>,
    config: PmtConfig,
    process: ProcessConfig,
}

impl LocalState {
    async fn process_event(&mut self, tsm: &mut TileSourceManager, event: Event) {
        if !matches!(
            event.kind,
            EventKind::Create(_)
                | EventKind::Remove(_)
                | EventKind::Modify(_)
                | EventKind::Access(AccessKind::Close(AccessMode::Write))
        ) {
            return;
        }

        let sources = match discover_sources_by_ext(
            &self.directories,
            &[PMTILES_EXT],
            &self.path_cache,
            &self.id_resolver,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("failed to rediscover sources from directories {e:?}");
                return;
            }
        };

        let prev: BTreeMap<String, u128> =
            self.sources.iter().map(|(k, v)| (k.clone(), v.1)).collect();
        let next: BTreeMap<String, u128> = sources.iter().map(|(k, v)| (k.clone(), v.1)).collect();
        let sources_clone = sources.clone();
        let config = self.config.clone();

        let adv = ReloadAdvisory::from_maps(
            &prev,
            &next,
            async move |id| -> MartinResult<BoxedSource> {
                let p = sources_clone
                    .get(&id)
                    .ok_or(MartinError::SourceNotFound(id.clone()))?;
                config.new_sources(id, p.0.clone(), p.2).await
            },
            self.process.clone(),
        )
        .await;

        match tsm.apply_changes(adv).await {
            Ok(()) => self.sources = sources,
            Err(e) => tracing::warn!("failed to apply reload changes: {e:?}"),
        }
    }
}

/// Mutable state owned by the remote-polling task.
struct RemoteState {
    id_resolver: IdResolver,
    remote_prefixes: Vec<Url>,
    remote_sources: BTreeMap<String, Url>,
    config: PmtConfig,
    process: ProcessConfig,
}

impl RemoteState {
    /// Polling tick: re-list every remote prefix and apply the diff against the previous
    /// `remote_sources` snapshot. Failures of individual prefixes are logged and skipped
    /// (a transient remote outage shouldn't flap the catalog).
    async fn tick(&mut self, tsm: &mut TileSourceManager) {
        let next = self.discover_remote_sources().await;

        let prev_ids: std::collections::BTreeSet<String> =
            self.remote_sources.keys().cloned().collect();
        let next_ids: std::collections::BTreeSet<String> = next.keys().cloned().collect();
        if prev_ids == next_ids {
            self.remote_sources = next;
            return;
        }

        let next_clone = next.clone();
        let config = self.config.clone();

        let adv = ReloadAdvisory::from_sets(
            &prev_ids,
            &next_ids,
            async move |id| -> MartinResult<BoxedSource> {
                let url = next_clone.get(&id).ok_or_else(|| {
                    MartinError::from(ConfigFileError::InvalidSourceFilePath(
                        id.clone(),
                        PathBuf::new(),
                    ))
                })?;
                config
                    .new_sources_url(id, url.clone(), CachePolicy::default())
                    .await
            },
            self.process.clone(),
        )
        .await;

        match tsm.apply_changes(adv).await {
            Ok(()) => self.remote_sources = next,
            Err(e) => tracing::warn!("PMTilesReloader: remote apply_changes failed: {e:?}"),
        }
    }

    /// Lists every remote prefix. Per-prefix list failures are logged and treated as
    /// "this prefix contributed nothing this tick" so a transient outage doesn't flap the
    /// catalog. The id-resolver is keyed on the absolute object URL so the same logical
    /// id is reused across ticks.
    async fn discover_remote_sources(&self) -> BTreeMap<String, Url> {
        let mut out: BTreeMap<String, Url> = BTreeMap::new();
        for prefix in &self.remote_prefixes {
            match list_remote_prefix(prefix, &self.config.options, &self.id_resolver).await {
                Ok(entries) => {
                    for (id, url) in entries {
                        out.insert(id, url);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "PMTilesReloader: list failed for {prefix}: {e:?}; skipping prefix this tick"
                    );
                }
            }
        }
        out
    }
}

async fn list_remote_prefix(
    prefix: &Url,
    options: &std::collections::HashMap<String, String>,
    id_resolver: &IdResolver,
) -> MartinResult<Vec<(String, Url)>> {
    let (store, base) = object_store::parse_url_opts(prefix, options)
        .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, prefix.to_string()))?;

    let mut out = Vec::new();
    let mut stream = store.list(Some(&base));
    while let Some(meta) = stream
        .try_next()
        .await
        .map_err(|e| ConfigFileError::ObjectStoreUrlParsing(e, prefix.to_string()))?
    {
        if !meta.location.as_ref().ends_with(PMTILES_EXT_DOT) {
            continue;
        }
        let stem = meta
            .location
            .filename()
            .and_then(|f| f.strip_suffix(PMTILES_EXT_DOT))
            .unwrap_or("_unknown");
        // `meta.location` is reported relative to the *store's* root (which is `/` for the
        // local backend and the bucket for s3/gs/azure), not relative to the prefix we
        // asked to list. Reconstruct the absolute URL from scheme + authority + location
        // so it round-trips through `new_sources_url`.
        let object_url_str = format!(
            "{}://{}/{}",
            prefix.scheme(),
            prefix.host_str().unwrap_or(""),
            meta.location
        );
        let Ok(object_url) = Url::parse(&object_url_str) else {
            tracing::warn!("cannot build absolute URL from {object_url_str}");
            continue;
        };
        let id = id_resolver.resolve(stem, object_url.to_string());
        out.push((id, object_url));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use insta::assert_yaml_snapshot;

    use super::*;
    use crate::config::file::pmtiles::DEFAULT_RELOAD_INTERVAL_SECS;
    use crate::config::file::{FileConfig, FileConfigSource, FileConfigSrc, OnInvalid};
    use crate::config::primitives::OptOneMany;

    fn make_reloader(config: &FileConfigEnum<PmtConfig>) -> PMTilesReloader {
        let tsm = TileSourceManager::new(None, OnInvalid::Warn);
        let resolver = IdResolver::new(&[]);
        PMTilesReloader::new(tsm, resolver, config, &ProcessConfig::default())
    }

    #[derive(serde::Serialize)]
    struct ReloaderSnapshot {
        local_dir_count: usize,
        remote_prefix_count: usize,
        remote_prefixes: Vec<String>,
        interval_secs: u64,
    }

    impl From<&PMTilesReloader> for ReloaderSnapshot {
        fn from(r: &PMTilesReloader) -> Self {
            Self {
                local_dir_count: r.directories.len(),
                remote_prefix_count: r.remote_prefixes.len(),
                remote_prefixes: r.remote_prefixes.iter().map(ToString::to_string).collect(),
                interval_secs: r.remote_poll_interval().as_secs(),
            }
        }
    }

    #[test]
    fn new_with_none_config_yields_default_interval() {
        let reloader = make_reloader(&FileConfigEnum::None);
        assert!(reloader.directories.is_empty());
        assert!(reloader.remote_prefixes.is_empty());
        assert_eq!(
            reloader.remote_poll_interval(),
            Duration::from_secs(DEFAULT_RELOAD_INTERVAL_SECS)
        );
    }

    #[test]
    fn new_partitions_local_and_remote_paths() {
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![
                PathBuf::from("s3://bucket-a/"),
                PathBuf::from("s3://bucket-b/folder/"),
                PathBuf::from("https://example.com/tiles/"),
            ]),
            sources: None,
            custom: PmtConfig {
                reload_interval_secs: 30,
                ..PmtConfig::default()
            },
        });
        assert_yaml_snapshot!(ReloaderSnapshot::from(&make_reloader(&cfg)), @r#"
        local_dir_count: 0
        remote_prefix_count: 3
        remote_prefixes:
          - "https://example.com/tiles/"
          - "s3://bucket-a/"
          - "s3://bucket-b/folder/"
        interval_secs: 30
        "#);
    }

    #[test]
    fn new_dedups_remote_prefixes() {
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::Many(vec![
                PathBuf::from("s3://bucket/"),
                PathBuf::from("s3://bucket/"),
            ]),
            sources: None,
            custom: PmtConfig::default(),
        });
        let r = make_reloader(&cfg);
        assert_eq!(r.remote_prefixes.len(), 1);
    }

    #[test]
    fn new_skips_remote_individually_configured_sources() {
        let mut sources: BTreeMap<String, FileConfigSrc> = BTreeMap::new();
        sources.insert(
            "remote_a".to_string(),
            FileConfigSrc::Obj(FileConfigSource {
                path: PathBuf::from("s3://bucket/file.pmtiles"),
                cache: CachePolicy::default(),
            }),
        );
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::NoVals,
            sources: Some(sources),
            custom: PmtConfig::default(),
        });
        let r = make_reloader(&cfg);
        // Remote single-file sources are tracked elsewhere (resolve_files) — the reloader
        // does not need to re-list them.
        assert!(r.sources.is_empty());
        assert!(r.remote_prefixes.is_empty());
    }
}
