use futures::future::{BoxFuture, join_all, ready};
use itertools::Itertools as _;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::duckdb::DuckDBPool;
use tracing::info;

use crate::config::file::tiles::duckdb::resolver::geoparquet::resolve_geoparquet_source;
use crate::config::file::tiles::duckdb::sources::{
    DuckDbDatabaseEntry, GeoParquetEntry, GeoParquetLocation,
};
use crate::config::file::tiles::duckdb::{DuckDbConfig, DuckDbSourceEntry};
use crate::config::file::{CachePolicy, ResolutionResult, TileSourceWarning};
use crate::config::primitives::IdResolver;

/// One resolved DuckDB source entry: a live source, or a per-source warning.
type ResolvedSource = BoxFuture<'static, Result<BoxedSource, TileSourceWarning>>;

fn resolve_database_entry(entry: &DuckDbDatabaseEntry, id_resolver: &IdResolver) -> ResolvedSource {
    let name = entry
        .database
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("duckdb");
    let source_id = id_resolver.resolve(name, entry.database.to_string_lossy().into_owned());
    Box::pin(ready(Err(TileSourceWarning::SourceError {
        source_id,
        error: "DuckDB database sources are not yet supported; entry skipped".into(),
    })))
}

fn resolve_geoparquet_entry(
    entry: &GeoParquetEntry,
    id_resolver: &IdResolver,
    default_cache: CachePolicy,
) -> ResolvedSource {
    let location = entry
        .location
        .as_ref()
        .expect("GeoParquetEntry must be finalized before resolve");
    let name = entry.layer_id.clone().unwrap_or_else(|| location.stem());
    let source_id = id_resolver.resolve(&name, location.to_source_string());
    let pool_size = entry
        .settings
        .pool_size
        .expect("pool_size must be set by DuckDbConfig::finalize")
        .get();

    let pool = match location {
        GeoParquetLocation::Local(path) => DuckDBPool::new_local_geoparquet(
            source_id.clone(),
            path.clone(),
            pool_size,
            entry.settings.threads,
            entry.settings.memory_limit_mb,
        ),
        GeoParquetLocation::Remote(url) => DuckDBPool::new_remote_geoparquet(
            source_id.clone(),
            url.clone(),
            pool_size,
            entry.settings.threads,
            entry.settings.memory_limit_mb,
        ),
    };
    let pool = match pool {
        Ok(pool) => pool,
        Err(error) => {
            return Box::pin(ready(Err(TileSourceWarning::SourceError {
                source_id,
                error: error.to_string(),
            })));
        }
    };

    let entry = entry.clone();
    Box::pin(async move {
        match resolve_geoparquet_source(source_id.clone(), &entry, pool, default_cache).await {
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
                    geoparquet: "../tests/fixtures/duckdb/geoparquet_polygons.parquet".into(),
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
        assert_eq!(warnings.len(), 1);
    }

    #[tokio::test]
    async fn missing_geoparquet_file_fails_finalize() {
        let mut cfg = DuckDbConfig {
            sources: vec![DuckDbSourceEntry::GeoParquet(GeoParquetEntry {
                geoparquet: "/no/such/file.parquet".into(),
                srid: Some(4326),
                ..GeoParquetEntry::default()
            })],
            ..DuckDbConfig::default()
        };
        let err = cfg.finalize().await.expect_err("missing file");
        assert!(
            err.to_string().contains("no/such/file.parquet")
                || err.to_string().contains("No such file"),
            "unexpected error: {err}"
        );
    }
}
