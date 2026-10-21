use std::collections::HashMap;

use futures::future::{BoxFuture, join_all, ready};
use itertools::Itertools as _;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::duckdb::DuckDBPool;
use tracing::info;

use crate::config::file::process::ProcessConfig;
use crate::config::file::tiles::duckdb::resolver::database::resolve_database_entry;
use crate::config::file::tiles::duckdb::resolver::geoparquet::resolve_geoparquet_source;
use crate::config::file::tiles::duckdb::sources::{GeoParquetEntry, GeoParquetLocation};
use crate::config::file::tiles::duckdb::{DuckDbConfig, DuckDbSourceEntry};
use crate::config::file::{CachePolicy, SourceBuildResult, TileSourceWarning};
use crate::config::primitives::IdResolver;

/// One resolved `DuckDB` source: a live source, or a per-source warning.
type ResolvedSource = BoxFuture<'static, Result<BoxedSource, TileSourceWarning>>;

/// Every source one config entry resolves to.
type ResolvedEntry = BoxFuture<'static, Vec<Result<BoxedSource, TileSourceWarning>>>;

pub type DuckDbResolution =
    SourceBuildResult<(Vec<BoxedSource>, Vec<TileSourceWarning>, HashMap<String, ProcessConfig>)>;

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
            Box::pin(async move { resolve_database_entry(&entry, &id_resolver, default_cache).await })
        }
        DuckDbSourceEntry::GeoParquet(entry) => {
            let source = resolve_geoparquet_entry(entry, id_resolver, default_cache);
            Box::pin(async move { vec![source.await] })
        }
    }
}

impl DuckDbConfig {
    /// Resolve configured `DuckDB` sources into live tile sources.
    pub async fn resolve(&self, id_resolver: IdResolver, default_cache: CachePolicy) -> DuckDbResolution {
        let default_cache = self.cache.or(&default_cache);
        let pending = self
            .sources
            .iter()
            .map(|source| resolve_source_entry(source, &id_resolver, default_cache.clone()))
            .collect::<Vec<_>>();
        let resolved = join_all(pending).await;
        let process = self
            .sources
            .iter()
            .zip(&resolved)
            .flat_map(|(source, results)| {
                let process = source.process_config();
                results
                    .iter()
                    .flatten()
                    .map(move |src| (src.get_id().to_owned(), process.clone()))
            })
            .collect();
        let (sources, warnings) = resolved.into_iter().flatten().partition_result();
        Ok((sources, warnings, process))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use insta::assert_yaml_snapshot;

    use super::*;
    use crate::config::file::ConfigurationLivecycleHooks as _;
    use crate::config::file::tiles::duckdb::sources::{
        DuckDbDatabaseEntry, DuckDbTableEntry, GeoParquetEntry, MvtLayerOptions,
    };
    use crate::test_support::duckdb::TestDatabase;

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

        let (sources, warnings, _) = cfg
            .resolve(IdResolver::default(), CachePolicy::default())
            .await
            .expect("resolution succeeds");

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(
            sources.iter().map(|source| source.get_id()).collect::<Vec<_>>(),
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

        let (sources, warnings, _) = cfg
            .resolve(IdResolver::default(), CachePolicy::default())
            .await
            .expect("resolution succeeds despite warnings");

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].get_id(), "geoparquet_polygons");
        assert_eq!(warnings.len(), 1);
        let TileSourceWarning::SourceError { source_id, error } = &warnings[0] else {
            panic!("expected SourceError, got {:?}", warnings[0]);
        };
        assert_eq!(source_id, "missing");
        assert!(error.contains("no_such_table"), "{error}");
    }

    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct DiscoverFixture {
        sql: String,
        config: String,
    }

    #[derive(serde::Serialize)]
    struct Discovered {
        sources: BTreeMap<String, String>,
        warnings: Vec<String>,
    }

    #[test]
    fn resolve_matches_each_fixture() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("build runtime");
        insta::glob!(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/duckdb_discover"),
            "*.yaml",
            |path| {
                let text = std::fs::read_to_string(path).expect("read fixture");
                let fixture: DiscoverFixture = serde_saphyr::from_str(&text).expect("parse fixture");
                let db = TestDatabase::from_sql("fixture.duckdb", &fixture.sql);
                let mut cfg: DuckDbConfig = serde_saphyr::from_str(&fixture.config).expect("parse config");
                for source in &mut cfg.sources {
                    if let DuckDbSourceEntry::Database(entry) = source {
                        entry.database = db.path().to_owned();
                    }
                }
                let (sources, warnings, _) = runtime.block_on(async {
                    cfg.finalize().await.expect("finalize");
                    cfg.resolve(IdResolver::default(), CachePolicy::default())
                        .await
                        .expect("resolution succeeds")
                });
                let db_path = db.path().to_string_lossy().into_owned();
                let discovered = Discovered {
                    sources: sources
                        .iter()
                        .map(|source| {
                            let tilejson = source.get_tilejson();
                            let description = tilejson.description.clone().unwrap_or_default();
                            let fields = tilejson
                                .vector_layers
                                .iter()
                                .flatten()
                                .flat_map(|layer| layer.fields.keys().cloned())
                                .collect::<Vec<_>>();
                            let description = format!("{description} fields [{}]", fields.join(", "));
                            (
                                source.get_id().to_owned(),
                                description.replace(&db_path, "fixture.duckdb"),
                            )
                        })
                        .collect(),
                    warnings: warnings
                        .iter()
                        .map(|warning| warning.to_string().replace(&db_path, "fixture.duckdb"))
                        .collect(),
                };
                assert_yaml_snapshot!(discovered);
            }
        );
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
            err.to_string().contains("no/such/file.parquet") || err.to_string().contains("No such file"),
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
            err.to_string().contains("no/such/file.duckdb") || err.to_string().contains("No such file"),
            "unexpected error: {err}"
        );
    }
}
