use std::collections::BTreeMap;
use std::path::PathBuf;

use martin_core::tiles::BoxedSource;
use martin_core::tiles::mbtiles::MbtSource;

use crate::config::file::driver::{NotifyTrigger, Sink as _, Trigger as _};
use crate::config::file::mbtiles::MbtConfig;
use crate::config::file::process::ProcessConfig;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
use crate::config::file::resolve_process_config;
use crate::config::file::tiles::reload::{discover_sources_by_ext, path_modified_ms};
use crate::config::file::{CachePolicy, FileConfigEnum};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::{MartinError, MartinResult, ReloadAdvisory, TileSourceManager};

pub struct MBTilesReloader {
    /// ID resolver that ensures a unique ID is assigned to each source.
    id_resolver: IdResolver,
    /// Tile Source Manager to which we should send `ReloadAdvisory` messages.
    tile_source_manager: TileSourceManager,
    /// Map of Source ID => (path, modified timestamp, cache policy).
    sources: BTreeMap<String, (PathBuf, u128, CachePolicy)>,
    /// Absolute path of all directories that are watched by this reloader.
    directories: Vec<PathBuf>,
    /// Maps canonical paths of explicitly configured sources to their cache policy,
    /// so that directory-discovered sources that match a configured path inherit its policy.
    path_cache: BTreeMap<PathBuf, CachePolicy>,
    /// Process config to apply to dynamically-discovered sources.
    /// Resolved from `mbtiles.convert_to_mlt` (source-type) > global `convert_to_mlt` > default.
    process: ProcessConfig,
}

impl MBTilesReloader {
    /// Creates a new `MBTilesReloader` from the given config.
    ///
    /// Snapshots the current modified timestamps of all explicitly configured sources so that
    /// changes can be detected on the first reload cycle. Collects all configured paths as
    /// directories to watch.
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<MbtConfig>,
        global_process: &ProcessConfig,
    ) -> Self {
        let mut sources: BTreeMap<String, (PathBuf, u128, CachePolicy)> = BTreeMap::new();
        let mut directories: Vec<PathBuf> = vec![];
        let mut path_cache: BTreeMap<PathBuf, CachePolicy> = BTreeMap::new();

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
            let _ = (config, global_process);
            ProcessConfig::default()
        };

        if let FileConfigEnum::Config(cfg) = config
            && let Some(s) = &cfg.sources
        {
            for (id, src) in s {
                let path = src.get_path();
                let policy = src.cache_zoom();
                let Ok(canonical) = path.canonicalize() else {
                    tracing::warn!(source.id = %id, path = ?path, "failed to resolve canonical path for tile source");
                    continue;
                };
                let Some(modified_ms) = path_modified_ms(path) else {
                    continue;
                };

                path_cache.insert(canonical.clone(), policy);
                sources.insert(id.clone(), (canonical.clone(), modified_ms, policy));
            }
        }

        let mut push_canonical = |path: &PathBuf| match path.canonicalize() {
            Ok(p) => directories.push(p),
            Err(e) => {
                tracing::warn!(directory = ?path, error = %e, "failed to canonicalize watch directory");
            }
        };

        match config {
            FileConfigEnum::Config(cfg) => match &cfg.paths {
                OptOneMany::One(path) => push_canonical(path),
                OptOneMany::Many(paths) => paths.iter().for_each(&mut push_canonical),
                OptOneMany::NoVals => {}
            },
            FileConfigEnum::Path(path) => push_canonical(path),
            FileConfigEnum::Paths(paths) => paths.iter().for_each(push_canonical),
            FileConfigEnum::None => {}
        }

        directories.sort();
        directories.dedup();

        Self {
            tile_source_manager: tsm,
            id_resolver,
            sources,
            directories,
            path_cache,
            process,
        }
    }

    /// Starts watching configured directories for `.mbtiles` file changes.
    ///
    /// Spawns a background task that listens for filesystem events and calls
    /// [`TileSourceManager::apply_changes`] with a [`ReloadAdvisory`] whenever sources are
    /// added, removed, or modified. Returns immediately after the watcher and task are started.
    /// Does nothing if no directories are configured.
    pub fn start(mut self) -> MartinResult<()> {
        if self.directories.is_empty() {
            return Ok(());
        }

        let mut trigger = NotifyTrigger::new(&self.directories)?;

        tokio::spawn(async move {
            let mut tsm = self.tile_source_manager.clone();
            self.seed_snapshot().await;

            while trigger.next().await.is_some() {
                self.process_event(&mut tsm).await;
            }
        });

        Ok(())
    }

    /// Merge directory-discovered files into `self.sources`. `new` only seeds explicit
    /// `cfg.sources`, so without this initial scan, removing or modifying a file that
    /// existed at startup produces an empty diff and the catalog drifts from disk.
    async fn seed_snapshot(&mut self) {
        match discover_sources_by_ext(
            &self.directories,
            &["mbtiles"],
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

    /// Rediscovers sources and applies any changes.
    ///
    /// Diff is computed via a fresh [`discover_sources_by_ext`] snapshot vs last known state.
    /// Logs and returns without updating state if rediscovery or [`Sink::apply_changes`] fails.
    async fn process_event(&mut self, tsm: &mut TileSourceManager) {
        let sources = match discover_sources_by_ext(
            &self.directories,
            &["mbtiles"],
            &self.path_cache,
            &self.id_resolver,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = ?e, "failed to rediscover sources from directories");
                return;
            }
        };

        let prev: BTreeMap<String, u128> = self
            .sources
            .iter()
            .map(|(k, v)| (k.clone(), v.1))
            .collect::<_>();
        let next: BTreeMap<String, u128> =
            sources.iter().map(|(k, v)| (k.clone(), v.1)).collect::<_>();
        let sources_clone = sources.clone();

        let adv = ReloadAdvisory::from_maps(
            &prev,
            &next,
            async move |id| -> MartinResult<BoxedSource> {
                let p = sources_clone
                    .get(&id)
                    .ok_or(MartinError::SourceNotFound(id.clone()))?;
                let src = MbtSource::new(id, p.0.clone(), p.2.zoom()).await?;

                Ok(Box::new(src) as BoxedSource)
            },
            self.process.clone(),
        )
        .await;

        match tsm.apply_changes(adv).await {
            Ok(()) => self.sources = sources,
            Err(e) => {
                tracing::warn!(error = ?e, "failed to apply reload changes");
            }
        }
    }
}
