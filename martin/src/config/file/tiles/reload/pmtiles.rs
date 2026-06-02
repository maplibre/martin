use std::collections::BTreeMap;
use std::path::PathBuf;
#[cfg(test)]
use std::time::Duration;

use futures::stream::TryStreamExt as _;
use martin_core::tiles::BoxedSource;
use object_store::ObjectStore as _;
use url::Url;

use crate::config::file::driver::{NotifyTrigger, PollTrigger, Sink as _, Trigger as _};
use crate::config::file::file_config::is_remote_url;
use crate::config::file::pmtiles::PmtConfig;
use crate::config::file::process::ProcessConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::resolve_process_config;
use crate::config::file::tiles::reload::{discover_sources_by_ext, path_modified_ms};
use crate::config::file::{
    CachePolicy, ConfigFileError, FileConfigEnum, TileSourceConfiguration as _,
};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::{MartinError, MartinResult, ReloadAdvisory, TileSourceManager};

const PMTILES_EXT: &str = "pmtiles";
const PMTILES_EXT_DOT: &str = ".pmtiles";

/// Reloader for `PMTiles` sources.
///
/// Local directories use `notify` for sub-second feedback; remote URL prefixes (`s3://`,
/// `gs://`, `https://`, …) fall back to polling because blob stores have no event channel.
pub struct PMTilesReloader {
    id_resolver: IdResolver,
    tile_source_manager: TileSourceManager,
    /// Last-known local source state, keyed on resolved id.
    sources: BTreeMap<String, (PathBuf, u128, CachePolicy)>,
    directories: Vec<PathBuf>,
    /// Cache policy by canonical path, so directory-discovered sources can inherit
    /// the policy of an explicitly configured one.
    path_cache: BTreeMap<PathBuf, CachePolicy>,
    config: PmtConfig,
    process: ProcessConfig,
    remote_prefixes: Vec<Url>,
    /// Last-known remote source state, keyed on resolved id.
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
        self.config.reload_interval
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
        let interval = config.reload_interval;

        // Local watcher and remote poller each own their state inside their spawned
        // task -- splitting the reloader avoids a shared mutex.
        if !directories.is_empty() {
            let mut trigger = NotifyTrigger::new(&directories)?;

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
                local_state.seed_snapshot().await;
                while trigger.next().await.is_some() {
                    local_state.process_event(&mut tsm_local).await;
                }
            });
        }

        if !remote_prefixes.is_empty() {
            if interval.is_zero() {
                tracing::info!(
                    "PMTilesReloader: remote prefix polling disabled (reload_interval = 0s)"
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
                let mut trigger = PollTrigger::new(interval);
                tokio::spawn(async move {
                    while trigger.next().await.is_some() {
                        remote_state.tick(&mut tsm_remote).await;
                    }
                });
            }
        }

        Ok(())
    }
}

/// State for the local-directory watcher task; lives inside the task so no mutex is needed.
struct LocalState {
    id_resolver: IdResolver,
    sources: BTreeMap<String, (PathBuf, u128, CachePolicy)>,
    directories: Vec<PathBuf>,
    path_cache: BTreeMap<PathBuf, CachePolicy>,
    config: PmtConfig,
    process: ProcessConfig,
}

impl LocalState {
    /// Merge directory-discovered files into `self.sources` so the first event-driven
    /// diff matches the catalog state `TileSourceManager` was populated with at startup.
    /// Without this, removing/modifying a pre-existing file produces an empty diff
    /// (`prev` is empty because `LocalState::new` only seeds explicit `cfg.sources`)
    /// and the catalog drifts from the filesystem.
    async fn seed_snapshot(&mut self) {
        match discover_sources_by_ext(
            &self.directories,
            &[PMTILES_EXT],
            &self.path_cache,
            &self.id_resolver,
        )
        .await
        {
            Ok(discovered) => {
                for (id, entry) in discovered {
                    self.sources.entry(id).or_insert(entry);
                }
            }
            Err(e) => {
                tracing::warn!("failed to seed reloader snapshot from directories {e:?}");
            }
        }
    }

    async fn process_event(&mut self, tsm: &mut TileSourceManager) {
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

/// State for the remote-polling task; lives inside the task so no mutex is needed.
struct RemoteState {
    id_resolver: IdResolver,
    remote_prefixes: Vec<Url>,
    remote_sources: BTreeMap<String, Url>,
    config: PmtConfig,
    process: ProcessConfig,
}

impl RemoteState {
    /// One polling pass: re-list every prefix, then diff against the prior snapshot.
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

    /// Per-prefix failures are logged and skipped so a transient outage doesn't flap
    /// the catalog.
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
        .map_err(|e| ConfigFileError::ObjectStoreList(e, prefix.to_string()))?
    {
        if !meta.location.as_ref().ends_with(PMTILES_EXT_DOT) {
            continue;
        }
        let stem = meta
            .location
            .filename()
            .and_then(|f| f.strip_suffix(PMTILES_EXT_DOT))
            .unwrap_or("_unknown");
        // `meta.location` is store-relative (bucket-rooted for s3/gs/azure), so we have
        // to reattach scheme+authority to round-trip through `new_sources_url`.
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
    use crate::config::file::pmtiles::DEFAULT_RELOAD_INTERVAL;
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
        assert_eq!(reloader.remote_poll_interval(), DEFAULT_RELOAD_INTERVAL);
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
                reload_interval: Duration::from_secs(30),
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
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                convert_to_mlt: None,
                #[cfg(all(feature = "mlt", feature = "_tiles"))]
                convert_to_mvt: None,
            }),
        );
        let cfg = FileConfigEnum::Config(FileConfig {
            paths: OptOneMany::NoVals,
            sources: Some(sources),
            custom: PmtConfig::default(),
        });
        let r = make_reloader(&cfg);
        // Remote single-file sources are tracked elsewhere (resolve_files) -- the reloader
        // does not need to re-list them.
        assert!(r.sources.is_empty());
        assert!(r.remote_prefixes.is_empty());
    }
}
