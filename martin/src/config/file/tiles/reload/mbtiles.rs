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

use crate::config::primitives::OptOneMany;
use crate::{
    MartinError, MartinResult, ReloadAdvisory, TileSourceManager, config::primitives::IdResolver,
};
use crate::config::file::{FileConfigEnum, mbtiles::MbtConfig};

pub struct MBTilesReloader {
    /// ID resolver that ensures a unique ID is assigned to each source.
    id_resolver: IdResolver,
    /// Map of Source ID => modified timestamp.
    sources: BTreeMap<String, (PathBuf, u128)>,
    /// Directories that are watched by this reloader.
    directories: Vec<PathBuf>
}

impl MBTilesReloader {
    pub fn new(id_resolver: IdResolver, config: &FileConfigEnum<MbtConfig>) -> Self {
        let mut sources: BTreeMap<String, (PathBuf, u128)> = BTreeMap::new();
        let mut directories: Vec<PathBuf> = vec![];
        match config {
            FileConfigEnum::Config(cfg) => {
                if let Some(s) = &cfg.sources {
                    for (id, src) in s {
                        let path = src.get_path();
                        let Ok(metadata) = path.metadata() else {
                            continue;
                        };
                        let Ok(modified) = metadata.modified() else {
                            continue;
                        };
                        let Ok(unix_epoch_delta) = modified.duration_since(UNIX_EPOCH) else {
                            continue;
                        };
                        sources.insert(id.clone(), (path.clone(), unix_epoch_delta.as_millis()));
                    }
                };
                match &cfg.paths {
                    OptOneMany::One(path) => {
                        directories.push(path.clone());
                    },
                    OptOneMany::Many(paths) => {
                        for path in paths {
                            directories.push(path.clone());
                        }
                    },
                    _ => {}
                };
            }
            _ => {},
        };

        MBTilesReloader {
            id_resolver,
            sources,
            directories,
        }
    }

    pub fn watch(mut self, tsm: TileSourceManager) -> MartinResult<()> {
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

        for dir in self.directories
            .iter()
            .map(|d| d.canonicalize())
            .flatten()
            .filter(|d| d.is_absolute())
        {
            watcher
                // FIXME: find a naming scheme for paths that makes sense under recursive and enable it
                .watch(&dir.clone(), notify::RecursiveMode::NonRecursive)
                .map_err(|e| MartinError::DirectoryWatchError(e.kind))?;
        }

        tokio::spawn(async move {
            let _watcher = watcher;
            let mut _tsm = tsm;

            while let Some(event) = rx.recv().await {
                self.process_event(&mut _tsm, event).await
            }
        });

        Ok(())
    }

    pub async fn discover_sources(
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
                    out.insert(id, (path, unix_epoch_delta.as_millis()));
                }
            }
        }

        Ok(out)
    }

    pub async fn process_event(
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
