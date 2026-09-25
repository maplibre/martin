use std::collections::BTreeMap;
use std::future::{Future, ready};
use std::io;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use crate::TileSourceManager;
use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::discovery::{BuiltSource, Discovered, Discovery, Version};
use crate::config::file::tiles::driver::{Baseline, NotifyTrigger, ReloadDriver};
use crate::config::file::tiles::duckdb::resolver::DuckDbSourceError;
use crate::config::file::tiles::duckdb::sources::{GeoParquetEntry, GeoParquetLocation};
use crate::config::file::tiles::duckdb::{DuckDbConfig, DuckDbSourceEntry};
use crate::config::file::{
    CachePolicy, ResolvedProcess, SourceBuildError, SourceBuildResult, TileSourceWarning,
};
use crate::config::primitives::IdResolver;

pub struct DuckDbReloader {
    local: ReloadDriver<DuckDbLocalDiscovery, TileSourceManager>,
}

impl DuckDbReloader {
    #[must_use]
    pub fn new(
        tsm: TileSourceManager,
        id_resolver: &IdResolver,
        config: &DuckDbConfig,
        default_cache: CachePolicy,
        global_process: &ProcessConfig,
    ) -> Self {
        let default_cache = config.cache.or(default_cache);

        let kind_process = ProcessConfig::layered(
            global_process,
            &config.process_config(),
            &ProcessConfig::default(),
        );

        let mut local_entries: Vec<(String, GeoParquetEntry)> = Vec::new();
        for source in &config.sources {
            let DuckDbSourceEntry::GeoParquet(gp) = source else {
                continue;
            };
            let Some(GeoParquetLocation::Local(_)) = &gp.location else {
                continue;
            };
            let location = gp.location.as_ref().expect("finalized");
            let name = gp.layer_id.clone().unwrap_or_else(|| location.stem());
            let id = id_resolver.resolve(&name, location.to_source_string());
            local_entries.push((id, (**gp).clone()));
        }

        let discovery = DuckDbLocalDiscovery::new(local_entries, default_cache, kind_process);
        Self {
            local: ReloadDriver::new(discovery, tsm),
        }
    }

    #[expect(
        clippy::unused_self,
        reason = "interface symmetry with other reloaders"
    )]
    pub fn init(&mut self) -> impl Future<Output = SourceBuildResult<Vec<TileSourceWarning>>> {
        ready(Ok(Vec::new()))
    }

    pub fn start(self) -> notify::Result<()> {
        let watch_dirs = self.local.discovery().parent_dirs();
        if watch_dirs.is_empty() {
            return Ok(());
        }
        let trigger = NotifyTrigger::new(&watch_dirs, false)?;
        self.local.spawn(trigger, Baseline::StartupResolved);
        Ok(())
    }
}

pub struct DuckDbLocalDiscovery {
    entries: Vec<(String, GeoParquetEntry)>,
    default_cache: CachePolicy,
    kind_process: ProcessConfig,
    kind_process_resolved: ResolvedProcess,
}

impl DuckDbLocalDiscovery {
    fn new(
        entries: Vec<(String, GeoParquetEntry)>,
        default_cache: CachePolicy,
        kind_process: ProcessConfig,
    ) -> Self {
        let kind_process_resolved = kind_process
            .resolve()
            .expect("DuckDB kind-level process config validated at startup");
        Self {
            entries,
            default_cache,
            kind_process,
            kind_process_resolved,
        }
    }

    pub fn parent_dirs(&self) -> Vec<PathBuf> {
        use std::collections::BTreeSet;
        self.entries
            .iter()
            .filter_map(|(_, entry)| {
                if let Some(GeoParquetLocation::Local(path)) = &entry.location {
                    path.parent().map(PathBuf::from)
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
}

impl Discovery for DuckDbLocalDiscovery {
    type Args = GeoParquetEntry;

    fn discover(&self) -> impl Future<Output = SourceBuildResult<Discovered<Self::Args>>> + Send {
        let mut sources = BTreeMap::new();
        for (id, entry) in &self.entries {
            let Some(GeoParquetLocation::Local(path)) = &entry.location else {
                unreachable!("DuckDbLocalDiscovery only holds local GeoParquet entries")
            };
            let modified = match path.metadata().and_then(|m| m.modified()) {
                Ok(modified) => modified,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return ready(Err(SourceBuildError::Io(error))),
            };
            let mtime = modified
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            sources.insert(id.clone(), (Version::Tracked(mtime), entry.clone()));
        }
        ready(Ok(Discovered::new(sources)))
    }

    async fn build(&self, id: &str, entry: &Self::Args) -> SourceBuildResult<BuiltSource> {
        use martin_core::tiles::duckdb::DuckDBPool;

        use crate::config::file::tiles::duckdb::resolver::resolve_geoparquet_source;

        let location = entry
            .location
            .as_ref()
            .expect("GeoParquetEntry must be finalized before build");
        let pool_size = entry
            .settings
            .pool_size
            .expect("pool_size set by DuckDbConfig::finalize")
            .get();

        let GeoParquetLocation::Local(path) = location else {
            unreachable!("DuckDbLocalDiscovery only holds local GeoParquet entries")
        };
        let pool = DuckDBPool::new_local_geoparquet(
            id.to_owned(),
            path.clone(),
            pool_size,
            entry.settings.threads,
            entry.settings.memory_limit_mb,
        )
        .map_err(DuckDbSourceError::Pool)?;

        let source =
            resolve_geoparquet_source(id.to_owned(), entry, pool, self.default_cache).await?;

        let per_source = entry.process_config();
        let resolved_process = if per_source == ProcessConfig::default() {
            None
        } else {
            Some(
                ProcessConfig::layered(&self.kind_process, &ProcessConfig::default(), &per_source)
                    .resolve()
                    .map_err(|e| e.for_source(id.to_owned()))?,
            )
        };

        Ok(BuiltSource {
            source,
            provenance: None,
            process: resolved_process,
        })
    }

    fn process(&self) -> ResolvedProcess {
        self.kind_process_resolved.clone()
    }
}
