use std::path::{Path, PathBuf};

use futures::future::{BoxFuture, join_all};
use itertools::Itertools as _;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::duckdb::DuckDBPool;
use tracing::info;
use url::Url;

use crate::config::file::tiles::duckdb::resolver::geoparquet::resolve_geoparquet_source;
use crate::config::file::tiles::duckdb::sources::{DuckDbDatabaseEntry, GeoParquetEntry};
use crate::config::file::tiles::duckdb::{DuckDbConfig, DuckDbSourceEntry};
use crate::config::file::{CachePolicy, ResolutionResult, TileSourceWarning};
use crate::config::primitives::IdResolver;

// One resolved DuckDB source entry: a live source, or a per-source warning.
type ResolvedSource = BoxFuture<'static, Result<BoxedSource, TileSourceWarning>>;

enum GeoParquetLocation { 
    Local(PathBuf),
    Remote(Url),
}

fn classify_geoparquet_path(path: &Path) -> Result<GeoParquetLocation, String> {
    let Some(raw) = path.to_str() else {
        return Err(format!(
            "GeoParquet path is not valid UTF-8: {}",
            path.display()
        ));
    };

    if let Ok(url) = Url::parse(raw)
        && matches!(url.scheme(), "http" | "https")
    {
        return Ok(GeoParquetLocation::Remote(url));
    }

    path.canonicalize()
        .map(GeoParquetLocation::Local)
        .map_err(|error| {
            format!(
                "Failed to canonicalize GeoParquet path '{}': {error}",
                path.display()
            )
        })
}

fn resolve_database_entry(entry: &DuckDbDatabaseEntry, id_resolver: &IdResolver) -> ResolvedSource {
    let name = entry
        .database
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("duckdb");
    let source_id = id_resolver.resolve(
        name,
        entry.database.to_string_lossy().into_owned(),
    );
    Box::pin(std::future::ready(Err(TileSourceWarning::SourceError {
        source_id,
        error: "DuckDB database sources are not yet supported; entry skipped".to_string(),
    })))
}

fn resolve_geoparquet_entry(
    entry: &GeoParquetEntry,
    id_resolver: &IdResolver,
    default_cache: CachePolicy,
) -> ResolvedSource {
    let name = entry
        .layer_id
        .clone()
        .unwrap_or_else(|| {
            entry
                .geoparquet
                .file_stem()
                .and_then(|value| value.to_str())
                .filter(|value| !value.is_empty())
                .unwrap_or("duckdb")
                .to_string()
        });
    let location = match classify_geoparquet_path(&entry.geoparquet) {
        Ok(location) => location,
        Err(error) => {
            // Reserve the ID before returning so collision suffixes stay deterministic.
            let source_id = id_resolver.resolve(&name, entry.geoparquet.to_string_lossy().into_owned());
            return Box::pin(std::future::ready(Err(TileSourceWarning::SourceError {
                source_id,
                error,
            })));
        }
    };
    let source_id = id_resolver.resolve(
        &name,
        match &location {
            GeoParquetLocation::Local(path) => path.to_string_lossy().into_owned(),
            GeoParquetLocation::Remote(url) => url.to_string(),
        },
    );
    let pool_size = entry
        .settings
        .pool_size
        .expect("pool_size must be set by DuckDbConfig::finalize")
        .get();
    let pool = match location {
        GeoParquetLocation::Local(path) => DuckDBPool::new_local_geoparquet(
            source_id.clone(),
            path,
            pool_size,
            entry.settings.threads,
            entry.settings.memory_limit_mb,
        ),
        GeoParquetLocation::Remote(url) => DuckDBPool::new_remote_geoparquet(
            source_id.clone(),
            url,
            pool_size,
            entry.settings.threads,
            entry.settings.memory_limit_mb,
        ),
    };
    let pool = match pool {
        Ok(pool) => pool,
        Err(error) => {
            return Box::pin(std::future::ready(Err(TileSourceWarning::SourceError {
                source_id,
                error: error.to_string(),
            })));
        }
    };

    let entry = entry.clone();
    let source_id_for_resolve = source_id.clone();
    Box::pin(async move {
        match resolve_geoparquet_source(source_id_for_resolve, &entry, pool, default_cache).await {
            Ok(source) => {
                info!(source.id = %source_id, "Configured DuckDB GeoParquet source");
                Ok(source)
            }
            Err(error) => Err(TileSourceWarning::SourceError {
                source_id,
                error: error.to_string(),
            }),
        }
    })
}

fn resolve_source_entry(
    source: &DuckDbSourceEntry,
    id_resolver: &IdResolver,
    default_cache: CachePolicy,
) -> ResolvedSource {
    match source {
        DuckDbSourceEntry::Database(entry) => resolve_database_entry(entry, id_resolver),
        DuckDbSourceEntry::GeoParquet(entry) => {
            resolve_geoparquet_entry(entry, id_resolver, default_cache)
        }
    }
}

impl DuckDbConfig {
    /// Resolve configured DuckDB sources into live tile sources.
    pub async fn resolve(
        &mut self,
        id_resolver: IdResolver,
        default_cache: CachePolicy,
    ) -> ResolutionResult {
        let pending = self
            .sources
            .iter()
            .map(|source| resolve_source_entry(source, &id_resolver, default_cache))
            .collect::<Vec<_>>();
        Ok(join_all(pending).await.into_iter().partition_result())
    }
}

#[cfg(test)]
mod tests {
    use martin_core::tiles::Source;

    use super::*;
    use crate::config::file::ConfigurationLivecycleHooks as _;
    use crate::config::file::tiles::duckdb::sources::{DuckDbDatabaseEntry, GeoParquetEntry};
    use crate::test_support::duckdb::polygons_parquet_path;

    #[tokio::test(flavor = "multi_thread")]
    async fn colliding_stems_get_suffixes_even_when_the_entries_fail_to_resolve() {
        let mut cfg = DuckDbConfig {
            sources: vec![
                DuckDbSourceEntry::Database(DuckDbDatabaseEntry {
                    database: "/a/tiles.duckdb".into(),
                    ..DuckDbDatabaseEntry::default()
                }),
                DuckDbSourceEntry::Database(DuckDbDatabaseEntry {
                    database: "/b/tiles.duckdb".into(),
                    ..DuckDbDatabaseEntry::default()
                }),
            ],
            ..DuckDbConfig::default()
        };
        cfg.finalize().await.expect("finalize");

        let (sources, warnings) = cfg
            .resolve(IdResolver::default(), CachePolicy::default())
            .await
            .expect("resolution succeeds with warnings");

        assert!(sources.is_empty());
        assert_eq!(
            warnings
                .iter()
                .map(|warning| match warning {
                    TileSourceWarning::SourceError { source_id, .. } => source_id.as_str(),
                    other => panic!("expected SourceError, got {other:?}"),
                })
                .collect::<Vec<_>>(),
            ["tiles", "tiles.1"]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failing_entry_warns_and_leaves_its_valid_siblings_resolved() {
        let mut cfg = DuckDbConfig {
            sources: vec![
                DuckDbSourceEntry::Database(DuckDbDatabaseEntry {
                    database: "/data/tiles.duckdb".into(),
                    ..DuckDbDatabaseEntry::default()
                }),
                DuckDbSourceEntry::GeoParquet(GeoParquetEntry {
                    geoparquet: polygons_parquet_path(),
                    srid: Some(4326),
                    ..GeoParquetEntry::default()
                }),
                DuckDbSourceEntry::GeoParquet(GeoParquetEntry {
                    geoparquet: "/no/such/file.parquet".into(),
                    srid: Some(4326),
                    ..GeoParquetEntry::default()
                }),
            ],
            ..DuckDbConfig::default()
        };
        cfg.finalize().await.expect("finalize");

        let (sources, warnings) = cfg
            .resolve(IdResolver::default(), CachePolicy::default())
            .await
            .expect("resolution succeeds despite warnings");

        assert_eq!(sources.len(), 1);
        assert_eq!(Source::get_id(sources[0].as_ref()), "geoparquet_polygons");
        assert_eq!(warnings.len(), 2);
    }
}
