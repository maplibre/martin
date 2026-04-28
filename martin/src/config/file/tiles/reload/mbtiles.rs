use std::{collections::BTreeMap, path::PathBuf};

use super::{discover_sources_by_ext, path_modified_ms};

use martin_core::tiles::{BoxedSource, mbtiles::MbtSource};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, Watcher as _,
    event::{AccessKind, AccessMode},
};
use tokio::sync::mpsc;

use crate::config::file::{CachePolicy, FileConfigEnum, mbtiles::MbtConfig};
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
    ) -> Self {
        let mut sources: BTreeMap<String, (PathBuf, u128, CachePolicy)> = BTreeMap::new();
        let mut directories: Vec<PathBuf> = vec![];
        let mut path_cache: BTreeMap<PathBuf, CachePolicy> = BTreeMap::new();

        if let FileConfigEnum::Config(cfg) = config
            && let Some(s) = &cfg.sources
        {
            for (id, src) in s {
                let path = src.get_path();
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

        let mut push_canonical = |path: &PathBuf| match path.canonicalize() {
            Ok(p) => directories.push(p),
            Err(e) => tracing::warn!("failed to canonicalize watch directory {:?}: {e}", path),
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
        for dir in &self.directories {
            watcher
                // FIXME: find a naming scheme for paths that makes sense under recursive and enable it
                .watch(dir, notify::RecursiveMode::NonRecursive)
                .map_err(|e| MartinError::DirectoryWatchError(e.kind))?;
        }

        tokio::spawn(async move {
            let _watcher = watcher;
            let mut tsm = self.tile_source_manager.clone();

            while let Some(event) = rx.recv().await {
                self.process_event(&mut tsm, event).await;
            }
        });

        Ok(())
    }

    /// Handles a filesystem event by rediscovering sources and applying any changes.
    ///
    /// Uses the event only as a trigger — the actual diff is computed by comparing a fresh
    /// directory scan against the last known state. Skips event kinds that cannot result in source
    /// changes. Logs and returns without updating state if rediscovery or
    /// [`TileSourceManager::apply_changes`] fails.
    async fn process_event(&mut self, tsm: &mut TileSourceManager, event: Event) -> () {
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
            &["mbtiles"],
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

        let prev: BTreeMap<String, u128> = self
            .sources
            .iter()
            .map(|(k, v)| (k.clone(), v.1))
            .collect::<_>();
        let next: BTreeMap<String, u128> =
            sources.iter().map(|(k, v)| (k.clone(), v.1)).collect::<_>();
        let sources_clone = sources.clone();

        let adv =
            ReloadAdvisory::from_maps(&prev, &next, async move |id| -> MartinResult<BoxedSource> {
                let p = sources_clone
                    .get(&id)
                    .ok_or(MartinError::SourceNotFound(id.clone()))?;
                let src = MbtSource::new(id, p.0.clone(), p.2.zoom()).await?;

                Ok(Box::new(src) as BoxedSource)
            })
            .await;

        match tsm.apply_changes(adv).await {
            Ok(()) => self.sources = sources,
            Err(e) => tracing::warn!("failed to apply reload changes: {e:?}"),
        }
    }
}
