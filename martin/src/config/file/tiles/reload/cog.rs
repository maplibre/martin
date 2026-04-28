use std::{collections::BTreeMap, path::PathBuf};

use super::{discover_sources_by_ext, path_modified_ms};

use martin_core::tiles::{BoxedSource, cog::CogSource};
use notify::{
    Config, Event, EventKind, RecommendedWatcher, Watcher as _,
    event::{AccessKind, AccessMode},
};
use tokio::sync::mpsc;

use crate::config::file::{CachePolicy, FileConfigEnum, cog::CogConfig};
use crate::config::primitives::{IdResolver, OptOneMany};
use crate::{MartinError, MartinResult, ReloadAdvisory, TileSourceManager};

pub struct COGReloader {
    id_resolver: IdResolver,
    tile_source_manager: TileSourceManager,
    sources: BTreeMap<String, (PathBuf, u128, CachePolicy)>,
    directories: Vec<PathBuf>,
    path_cache: BTreeMap<PathBuf, CachePolicy>,
}

impl COGReloader {
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: IdResolver,
        config: &FileConfigEnum<CogConfig>,
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

    async fn discover_sources(
        &self,
    ) -> MartinResult<BTreeMap<String, (PathBuf, u128, CachePolicy)>> {
        discover_sources_by_ext(
            &self.directories,
            &["tif", "tiff"],
            &self.path_cache,
            &self.id_resolver,
        )
        .await
    }

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

        let sources = match self.discover_sources().await {
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
            .collect();
        let next: BTreeMap<String, u128> =
            sources.iter().map(|(k, v)| (k.clone(), v.1)).collect();
        let sources_clone = sources.clone();

        let adv =
            ReloadAdvisory::from_maps(&prev, &next, async move |id| -> MartinResult<BoxedSource> {
                let p = sources_clone
                    .get(&id)
                    .ok_or(MartinError::SourceNotFound(id.clone()))?;
                let src = CogSource::new(id, p.0.clone(), p.2.zoom())?;
                Ok(Box::new(src) as BoxedSource)
            })
            .await;

        match tsm.apply_changes(adv).await {
            Ok(()) => self.sources = sources,
            Err(e) => tracing::warn!("failed to apply reload changes: {e:?}"),
        }
    }
}
