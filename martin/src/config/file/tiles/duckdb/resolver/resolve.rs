use futures::future::{BoxFuture, join_all, ready};
use itertools::Itertools as _;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::duckdb::DuckDBPool;
use tracing::info;

use crate::config::file::tiles::duckdb::resolver::database::resolve_database_entry;
use crate::config::file::tiles::duckdb::resolver::geoparquet::resolve_geoparquet_source;
use crate::config::file::tiles::duckdb::sources::{GeoParquetEntry, GeoParquetLocation};
use crate::config::file::tiles::duckdb::{DuckDbConfig, DuckDbSourceEntry};
use crate::config::file::{CachePolicy, ResolutionResult, TileSourceWarning};
use crate::config::primitives::IdResolver;

/// One resolved `DuckDB` source: a live source, or a per-source warning.
type ResolvedSource = BoxFuture<'static, Result<BoxedSource, TileSourceWarning>>;

/// Every source one config entry resolves to.
type ResolvedEntry = BoxFuture<'static, Vec<Result<BoxedSource, TileSourceWarning>>>;

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
) -> ResolvedEntry {
    match source {
        DuckDbSourceEntry::Database(entry) => {
            let entry = entry.clone();
            let id_resolver = id_resolver.clone();
            Box::pin(
                async move { resolve_database_entry(&entry, &id_resolver, default_cache).await },
            )
        }
        DuckDbSourceEntry::GeoParquet(entry) => {
            let source = resolve_geoparquet_entry(entry, id_resolver, default_cache);
            Box::pin(async move { vec![source.await] })
        }
    }
}

impl DuckDbConfig {
    /// Resolve configured `DuckDB` sources into live tile sources.
    pub async fn resolve(
        &mut self,
        id_resolver: IdResolver,
        default_cache: CachePolicy,
    ) -> ResolutionResult {
        let default_cache = self.cache.or(default_cache);
        let pending = self
            .sources
            .iter()
            .map(|source| resolve_source_entry(source, &id_resolver, default_cache))
            .collect::<Vec<_>>();
        Ok(join_all(pending)
            .await
            .into_iter()
            .flatten()
            .partition_result())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use martin_core::tiles::Source;

    use super::*;
    use crate::config::file::ConfigurationLivecycleHooks as _;
    use crate::config::file::tiles::duckdb::sources::{
        DuckDbDatabaseEntry, DuckDbTableEntry, GeoParquetEntry, MvtLayerOptions,
    };

    const DATABASE_FIXTURE: &str = "../tests/fixtures/duckdb/database.duckdb";

    fn database_with_table(source_id: &str, schema: &str, table: &str) -> DuckDbSourceEntry {
        DuckDbSourceEntry::Database(Box::new(DuckDbDatabaseEntry {
            database: DATABASE_FIXTURE.into(),
            tables: Some(BTreeMap::from([(
                source_id.to_owned(),
                DuckDbTableEntry {
                    schema: Some(schema.to_owned()),
                    table: table.to_owned(),
                    layer: MvtLayerOptions {
                        srid: Some(4326),
                        ..MvtLayerOptions::default()
                    },
                    ..DuckDbTableEntry::default()
                },
            )])),
            ..DuckDbDatabaseEntry::default()
        }))
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn colliding_table_ids_across_database_entries_get_suffixes() {
        let mut cfg = DuckDbConfig {
            sources: vec![
                database_with_table("polygons", "main", "polygons"),
                database_with_table("polygons", "places", "points"),
            ],
            ..DuckDbConfig::default()
        };
        cfg.finalize().await.expect("finalize");

        let (sources, warnings) = cfg
            .resolve(IdResolver::default(), CachePolicy::default())
            .await
            .expect("resolution succeeds");

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            sources
                .iter()
                .map(|source| Source::get_id(source.as_ref()))
                .collect::<Vec<_>>(),
            ["polygons", "polygons.1"]
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failing_entry_warns_and_leaves_its_valid_siblings_resolved() {
        let mut cfg = DuckDbConfig {
            sources: vec![
                database_with_table("missing", "main", "no_such_table"),
                DuckDbSourceEntry::GeoParquet(Box::new(GeoParquetEntry {
                    geoparquet: "../tests/fixtures/duckdb/geoparquet_polygons.parquet".into(),
                    layer: MvtLayerOptions {
                        srid: Some(4326),
                        ..MvtLayerOptions::default()
                    },
                    ..GeoParquetEntry::default()
                })),
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
        let TileSourceWarning::SourceError { source_id, error } = &warnings[0] else {
            panic!("expected SourceError, got {:?}", warnings[0]);
        };
        assert_eq!(source_id, "missing");
        assert!(error.contains("no_such_table"), "{error}");
    }

    #[tokio::test]
    async fn missing_geoparquet_file_fails_finalize() {
        let mut cfg = DuckDbConfig {
            sources: vec![DuckDbSourceEntry::GeoParquet(Box::new(GeoParquetEntry {
                geoparquet: "/no/such/file.parquet".into(),
                layer: MvtLayerOptions {
                    srid: Some(4326),
                    ..MvtLayerOptions::default()
                },
                ..GeoParquetEntry::default()
            }))],
            ..DuckDbConfig::default()
        };
        let err = cfg.finalize().await.expect_err("missing file");
        assert!(
            err.to_string().contains("no/such/file.parquet")
                || err.to_string().contains("No such file"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn missing_database_file_fails_finalize() {
        let mut cfg = DuckDbConfig {
            sources: vec![DuckDbSourceEntry::Database(Box::new(DuckDbDatabaseEntry {
                database: "/no/such/file.duckdb".into(),
                ..DuckDbDatabaseEntry::default()
            }))],
            ..DuckDbConfig::default()
        };
        let err = cfg.finalize().await.expect_err("missing file");
        assert!(
            err.to_string().contains("no/such/file.duckdb")
                || err.to_string().contains("No such file"),
            "unexpected error: {err}"
        );
    }
}
