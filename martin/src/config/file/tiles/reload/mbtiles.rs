use std::{collections::BTreeMap, path::PathBuf, time::UNIX_EPOCH};

use martin_core::tiles::{mbtiles::MbtSource};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, Watcher,
    event::{AccessKind, AccessMode},
};
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::{
    MartinError, MartinResult, ReloadAdvisory, TileSourceManager,
    config::primitives::IdResolver,
};

pub struct MBTilesReloader {
    /// ID resolver that ensures a unique ID is assigned to each source.
    id_resolver: IdResolver,
    /// Map of Source ID => modified timestamp.
    sources: BTreeMap<String, u64>,
    /// Map of Source ID => absolute file path.
    paths: BTreeMap<String, PathBuf>,
}

impl MBTilesReloader {
    pub fn new(id_resolver: IdResolver, initial_sources: BTreeMap<String, u64>) -> Self {
        MBTilesReloader {
            id_resolver: id_resolver,
            sources: initial_sources,
            paths: BTreeMap::new(),
        }
    }

    pub fn watch(mut self, _tsm: &TileSourceManager, directories: Vec<PathBuf>) -> MartinResult<()> {
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

        for dir in directories.iter().map(|d| d.canonicalize()).flatten() {
            info!("watching {dir:?} for changes");
            watcher
                .watch(&dir.clone(), notify::RecursiveMode::NonRecursive)
                .map_err(|e| MartinError::DirectoryWatchError(e.kind))?;
        }

        tokio::spawn(async move {
            let _watcher = watcher;

            while let Some(event) = rx.recv().await {
                // rediscover in the configured paths. do not use the event for anything other than notification.
                let should_discover = match event.kind {
                    EventKind::Create(_)
                    | EventKind::Remove(_)
                    | EventKind::Modify(_)
                    | EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
                    _ => false,
                };
                if !should_discover {
                    continue;
                }

                let (versions, paths) = match self.discover_sources(&directories).await {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("failed to rediscover sources from directories {e:?}");
                        continue;
                    }
                };
                self.paths = paths;

                let mut adv = ReloadAdvisory::from_maps(&self.sources, &versions);
                for addition in adv.additions.iter_mut() {
                    if let Some(path) = &self.paths.get(&addition.name) {
                        if let Ok(src) =
                            MbtSource::new(addition.name.to_string(), path.to_path_buf()).await
                        {
                            addition.source = Some(Box::new(src));
                        }
                    }
                }
                for update in adv.updates.iter_mut() {
                    if let Some(path) = &self.paths.get(&update.name) {
                        if let Ok(src) =
                            MbtSource::new(update.name.to_string(), path.to_path_buf()).await
                        {
                            update.source = Some(Box::new(src));
                        }
                    }
                }
                info!("produced advisory {adv:?}")
            }
        });

        Ok(())
    }

    // pub async fn resolve_discoveries(&mut self, discoveries: &Vec<Discovery>) -> MartinResult<BTreeMap<String, (PathBuf, u64)>> {
    //     let new_map: BTreeMap<String, (PathBuf, u64)> = BTreeMap::new();
    //     for discovery in discoveries {
    //     }
    //     return Ok(new_map)
    // }

    pub async fn discover_sources(
        &mut self,
        directories: &Vec<PathBuf>,
    ) -> MartinResult<(BTreeMap<String, u64>, BTreeMap<String, PathBuf>)> {
        let mut versions: BTreeMap<String, u64> = BTreeMap::new();
        let mut paths: BTreeMap<String, PathBuf> = BTreeMap::new();

        for directory in directories {
            for result in directory.read_dir().map_err(|e| MartinError::IoError(e))? {
                let entry = result.map_err(|e| MartinError::IoError(e))?;
                let path = entry
                    .path()
                    .canonicalize()
                    .map_err(|e| MartinError::IoError(e))?;

                if path.is_file() && path.extension().is_some_and(|e| e == "mbtiles") {
                    let Some(stem) = path.file_stem().and_then(|o| o.to_str()) else {
                        continue;
                    };
                    let Ok(path_str) = path.clone().into_os_string().into_string() else {
                        continue;
                    };
                    let Ok(metadata) = path.clone().metadata() else {
                        continue;
                    };
                    let Ok(modified) = metadata.modified() else {
                        continue;
                    };
                    let Ok(unix_epoch_delta) = modified.duration_since(UNIX_EPOCH) else {
                        continue;
                    };

                    let id = self.id_resolver.resolve(stem, path_str.clone());
                    versions.insert(id.clone(), unix_epoch_delta.as_millis() as u64);
                    paths.insert(id, path);
                }
            }
        }

        Ok((versions, paths))
    }
}
