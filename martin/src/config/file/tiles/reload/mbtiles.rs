use std::{collections::BTreeMap, path::PathBuf, time::UNIX_EPOCH};

use martin_core::CacheZoomRange;
use martin_core::tiles::{BoxedSource, mbtiles::MbtSource};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, Watcher,
    event::{AccessKind, AccessMode},
};
use tokio::fs;
use tokio::sync::mpsc;
use tracing::warn;

use crate::config::primitives::{IdResolver, OptOneMany};
use crate::{
    MartinError, MartinResult, ReloadAdvisory, TileSourceManager,
};
use crate::config::file::{FileConfigEnum, mbtiles::MbtConfig};

pub struct MBTilesReloader {
    /// ID resolver that ensures a unique ID is assigned to each source.
    id_resolver: IdResolver,
    /// Tile Source Manager to which we should send ReloadAdvisory messages.
    tile_source_manager: TileSourceManager,
    /// Map of Source ID => modified timestamp.
    sources: BTreeMap<String, (PathBuf, u128)>,
    /// Absolute path of all directories that are watched by this reloader.
    directories: Vec<PathBuf>
}

impl MBTilesReloader {
    /// Creates a new `MBTilesReloader` from the given config.
    ///
    /// Snapshots the current modified timestamps of all explicitly configured sources so that
    /// changes can be detected on the first reload cycle. Collects all configured paths as
    /// directories to watch.
    pub fn new(tsm: TileSourceManager, id_resolver: IdResolver, config: &FileConfigEnum<MbtConfig>) -> Self {
        let mut sources: BTreeMap<String, (PathBuf, u128)> = BTreeMap::new();
        let mut directories: Vec<PathBuf> = vec![];

        if let FileConfigEnum::Config(cfg) = config {
            if let Some(s) = &cfg.sources {
                for (id, src) in s {
                    let path = src.get_path();
                    let Ok(metadata) = path.metadata() else {
                        tracing::warn!("failed to resolve metadata for {:?}", path);
                        continue;
                    };
                    let Ok(modified) = metadata.modified() else {
                        tracing::warn!("failed to resolve modified timestamp for {:?}", path);
                        continue;
                    };
                    let Ok(unix_epoch_delta) = modified.duration_since(UNIX_EPOCH) else {
                        tracing::warn!("failed to resolve difference between modified timestamp and unix epoch for {:?}", path);
                        continue;
                    };

                    sources.insert(id.clone(), (path.clone(), unix_epoch_delta.as_millis()));
                }
            };
        };

        let mut push_canonical = |path: &PathBuf| {
            match path.canonicalize() {
                Ok(p) => directories.push(p),
                Err(e) => tracing::warn!("failed to canonicalize watch directory {:?}: {e}", path),
            }
        };

        match config {
            FileConfigEnum::Config(cfg) => {
                match &cfg.paths {
                    OptOneMany::One(path) => push_canonical(path),
                    OptOneMany::Many(paths) => paths.iter().for_each(|p| push_canonical(p)),
                    _ => {}
                };
            },
            FileConfigEnum::Path(path) => push_canonical(path),
            FileConfigEnum::Paths(paths) => paths.iter().for_each(|p| push_canonical(p)),
            FileConfigEnum::None => {},
        }

        directories.sort();
        directories.dedup();

        MBTilesReloader {
            tile_source_manager: tsm,
            id_resolver,
            sources,
            directories,
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
            return Ok(())
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
            let mut _tsm = self.tile_source_manager.clone();

            while let Some(event) = rx.recv().await {
                self.process_event(&mut _tsm, event).await
            }
        });

        Ok(())
    }

    async fn discover_sources(
        &self,
    ) -> MartinResult<BTreeMap<String, (PathBuf, u128)>> {
        let mut out: BTreeMap<String, (PathBuf, u128)> = BTreeMap::new();

        for directory in &self.directories {
            let mut entries = fs::read_dir(directory).await.map_err(|e| MartinError::IoError(e))?;
            while let Some(entry) = entries.next_entry().await.map_err(|e| MartinError::IoError(e))? {
                let path = entry
                    .path()
                    .canonicalize()
                    .map_err(|e| MartinError::IoError(e))?;

                if path.is_file() && path.extension().is_some_and(|e| e == "mbtiles") {
                    let Some(stem) = path.file_stem().and_then(|o| o.to_str()) else {
                        tracing::warn!("failed to resolve file stem for path {:?}", path);
                        continue;
                    };
                    let Ok(path_str) = path.clone().into_os_string().into_string() else {
                        tracing::warn!("failed to resolve path string for path {:?}", path);
                        continue;
                    };
                    let Ok(metadata) = path.clone().metadata() else {
                        tracing::warn!("failed to metadata for path {:?}", path);
                        continue;
                    };
                    let Ok(modified) = metadata.modified() else {
                        tracing::warn!("failed to modified timestamp for path {:?}", path);
                        continue;
                    };
                    let Ok(unix_epoch_delta) = modified.duration_since(UNIX_EPOCH) else {
                        tracing::warn!("failed to resolve difference between modified timestamp and unix epoch for {:?}", path);
                        continue;
                    };

                    let id = self.id_resolver.resolve(stem, path_str.clone());
                    out.insert(id, (path, unix_epoch_delta.as_millis()));
                }
            }
        }

        Ok(out)
    }

    async fn process_event(
        &mut self,
        tsm: &mut TileSourceManager,
        event: Event
    ) -> () {
        // rediscover in the configured paths.
        // do not use the event for anything other than notification.
        let should_discover = match event.kind {
            EventKind::Create(_)
            | EventKind::Remove(_)
            | EventKind::Modify(_)
            | EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
            _ => false,
        };
        if !should_discover {
            return;
        }

        let sources = match self.discover_sources().await {
            Ok(v) => v,
            Err(e) => {
                warn!("failed to rediscover sources from directories {e:?}");
                return;
            }
        };

        let prev: BTreeMap<String, u128> = (&self.sources)
            .into_iter()
            .map(|(k, v)| (k.clone(), v.1))
            .collect::<_>();
        let next: BTreeMap<String, u128> = (&sources)
            .into_iter()
            .map(|(k, v)| (k.clone(), v.1))
            .collect::<_>();
        let sources_clone = sources.clone();

        let adv = ReloadAdvisory::from_maps(
            &prev,
            &next,
            async move |id| -> MartinResult<BoxedSource> {
                let p = sources_clone.get(&id).ok_or(
                    MartinError::DirectoryWatchError(notify::ErrorKind::Generic(
                        format!("Source {id} not found in discovered sources"),
                    )),
                )?;
                let src = MbtSource::new(id, p.0.clone(), CacheZoomRange::default()).await?;

                Ok(Box::new(src) as BoxedSource)
            },
        )
        .await;

        match tsm.apply_changes(adv).await {
            Ok(()) => self.sources = sources,
            Err(e) => warn!("failed to apply reload changes: {e:?}"),
        }
    }
}
