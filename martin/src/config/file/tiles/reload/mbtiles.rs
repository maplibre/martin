use std::{collections::BTreeMap, path::PathBuf, time::UNIX_EPOCH};

use martin_core::tiles::{BoxedSource, mbtiles::MbtSource};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, Watcher,
    event::{AccessKind, AccessMode},
};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{
    MartinError, MartinResult, ReloadAdvisory, TileSourceManager, config::primitives::IdResolver,
};

pub struct MBTilesReloader {
    /// ID resolver that ensures a unique ID is assigned to each source.
    id_resolver: IdResolver,
    /// Map of Source ID => modified timestamp.
    sources: BTreeMap<String, (PathBuf, u64)>,
}

impl MBTilesReloader {
    pub fn new(id_resolver: IdResolver, initial_sources: BTreeMap<String, (PathBuf, u64)>) -> Self {
        MBTilesReloader {
            id_resolver: id_resolver,
            sources: initial_sources,
        }
    }

    pub fn watch(mut self, tsm: TileSourceManager, directories: Vec<PathBuf>) -> MartinResult<()> {
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

        for dir in directories
            .iter()
            .map(|d| d.canonicalize())
            .flatten()
            .filter(|d| d.is_absolute())
        {
            watcher
                .watch(&dir.clone(), notify::RecursiveMode::NonRecursive)
                .map_err(|e| MartinError::DirectoryWatchError(e.kind))?;
        }

        tokio::spawn(async move {
            let _watcher = watcher;
            let _tsm = tsm;

            while let Some(event) = rx.recv().await {
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
                    continue;
                }

                let sources = match self.discover_sources(&directories).await {
                    Ok(v) => v,
                    Err(e) => {
                        warn!("failed to rediscover sources from directories {e:?}");
                        continue;
                    }
                };

                let prev: BTreeMap<String, u64> = (&self.sources)
                    .into_iter()
                    .map(|(k, v)| (k.clone(), v.1))
                    .collect::<_>();
                let next: BTreeMap<String, u64> = (&sources)
                    .into_iter()
                    .map(|(k, v)| (k.clone(), v.1))
                    .collect::<_>();
                self.sources = sources;
                let sources_clone = self.sources.clone();

                let adv = ReloadAdvisory::from_maps(
                    &prev,
                    &next,
                    async move |id| -> Option<BoxedSource> {
                        let Some(p) = sources_clone.get(&id) else {
                            return None;
                        };
                        let Ok(src) = MbtSource::new(id, p.0.clone()).await else {
                            return None;
                        };

                        Some(Box::new(src) as BoxedSource)
                    },
                )
                .await;

                _tsm.apply_changes(adv).await
            }
        });

        Ok(())
    }

    pub async fn discover_sources(
        &mut self,
        directories: &Vec<PathBuf>,
    ) -> MartinResult<BTreeMap<String, (PathBuf, u64)>> {
        let mut out: BTreeMap<String, (PathBuf, u64)> = BTreeMap::new();

        for directory in directories {
            for result in directory.read_dir().map_err(|e| MartinError::IoError(e))? {
                let entry = result.map_err(|e| MartinError::IoError(e))?;
                let path = entry
                    .path()
                    .canonicalize()
                    .map_err(|e| MartinError::IoError(e))?;

                if path.is_file() && path.extension().is_some_and(|e| e == "mbtiles") {
                    if !path.is_absolute() {
                        continue;
                    };
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
                    out.insert(id, (path, unix_epoch_delta.as_millis() as u64));
                }
            }
        }

        Ok(out)
    }
}
