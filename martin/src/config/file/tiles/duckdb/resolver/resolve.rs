use std::path::{Path, PathBuf};

use futures::future::join_all;
use martin_core::tiles::BoxedSource;
use martin_core::tiles::duckdb::DuckDBPool;
use tracing::info;
use url::Url;

use crate::config::file::tiles::duckdb::resolver::geoparquet::resolve_geoparquet_source;
use crate::config::file::tiles::duckdb::{DuckDbConfig, DuckDbSourceEntry};
use crate::config::file::{CachePolicy, ResolutionResult, TileSourceWarning};
use crate::config::primitives::IdResolver;

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

impl DuckDbConfig {
    /// Resolve configured DuckDB sources into live tile sources.
    pub async fn resolve(
        &mut self,
        id_resolver: IdResolver,
        default_cache: CachePolicy,
    ) -> ResolutionResult {
        let mut early_warnings = Vec::new();
        let mut pending = Vec::new();

        for (index, source) in self.sources.iter().enumerate() {
            match source {
                DuckDbSourceEntry::Database(entry) => {
                    let name = entry
                        .database
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .filter(|value| !value.is_empty())
                        .unwrap_or("duckdb");
                    let source_id =
                        id_resolver.resolve(name, entry.database.to_string_lossy().into_owned());
                    early_warnings.push((
                        index,
                        TileSourceWarning::SourceError {
                            source_id,
                            error: "DuckDB database sources are not yet supported; entry skipped"
                                .to_string(),
                        },
                    ));
                }
                DuckDbSourceEntry::GeoParquet(entry) => {
                    let name = entry.layer_id.clone().unwrap_or_else(|| {
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
                            // Reserve the ID before continuing so collision suffixes stay
                            // deterministic even when this source cannot be opened.
                            let source_id = id_resolver
                                .resolve(&name, entry.geoparquet.to_string_lossy().into_owned());
                            early_warnings
                                .push((index, TileSourceWarning::SourceError { source_id, error }));
                            continue;
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
                            early_warnings.push((
                                index,
                                TileSourceWarning::SourceError {
                                    source_id,
                                    error: error.to_string(),
                                },
                            ));
                            continue;
                        }
                    };
                    let entry = entry.clone();
                    pending.push(async move {
                        let result = resolve_geoparquet_source(
                            source_id.clone(),
                            &entry,
                            pool,
                            default_cache,
                        )
                        .await;
                        (index, source_id, result)
                    });
                }
            }
        }

        let mut ordered: Vec<Option<Result<BoxedSource, TileSourceWarning>>> =
            (0..self.sources.len()).map(|_| None).collect();
        for (index, warning) in early_warnings {
            ordered[index] = Some(Err(warning));
        }
        for (index, source_id, result) in join_all(pending).await {
            ordered[index] = Some(match result {
                Ok(source) => {
                    info!(source.id = %source_id, "Configured DuckDB GeoParquet source");
                    Ok(source)
                }
                Err(error) => Err(TileSourceWarning::SourceError {
                    source_id,
                    error: error.to_string(),
                }),
            });
        }

        let mut sources = Vec::new();
        let mut warnings = Vec::new();
        for result in ordered {
            match result.expect("every DuckDB source index must be filled") {
                Ok(source) => sources.push(source),
                Err(warning) => warnings.push(warning),
            }
        }
        Ok((sources, warnings))
    }
}

#[cfg(test)]
mod tests {
    use martin_core::tiles::Source;

    use super::*;
    use crate::config::file::ConfigurationLivecycleHooks as _;
    use crate::config::file::tiles::duckdb::sources::{DuckDbDatabaseEntry, GeoParquetEntry};
    use crate::test_support::duckdb::polygons_parquet_path;

    // IDs are assigned in config order, and colliding stems get deterministic suffixes even
    // when the entries themselves fail to resolve.
    #[tokio::test(flavor = "multi_thread")]
    async fn assigns_deterministic_ids_in_config_order() {
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

    // A per-source failure produces a warning without discarding a valid sibling, and
    // resolution still returns `Ok` (leaving `on_invalid` as the final policy gate).
    #[tokio::test(flavor = "multi_thread")]
    async fn warnings_do_not_discard_valid_sibling() {
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
