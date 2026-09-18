use martin_core::tiles::BoxedSource;
use martin_core::tiles::duckdb::{DuckDBPool, DuckDBSource, DuckDBSqlInfo};
use martin_tile_utils::{Encoding, Format, TileInfo};
use tracing::debug;

use super::covering::query_covering;
use crate::config::args::BoundsCalcType;
use crate::config::file::CachePolicy;
use crate::config::file::tiles::duckdb::resolver::bounds::bounds_with_auto;
use crate::config::file::tiles::duckdb::resolver::errors::DuckDbSourceResult;
use crate::config::file::tiles::duckdb::resolver::introspect::introspect;
use crate::config::file::tiles::duckdb::resolver::metadata::build_tilejson;
use crate::config::file::tiles::duckdb::resolver::sql::build_mvt_sql;
use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;
use crate::config::file::tiles::duckdb::sql_utils::escape_sql_string;

/// The finalized location as a `DuckDB` string literal, for functions that take a path.
fn geoparquet_source_literal(entry: &GeoParquetEntry) -> String {
    escape_sql_string(
        &entry
            .location
            .as_ref()
            .expect("GeoParquetEntry must be finalized before resolve")
            .to_source_string(),
    )
}

/// Builds the `DuckDB` `FROM` expression from the finalized location.
pub(crate) fn geoparquet_from_expr(entry: &GeoParquetEntry) -> (String, String) {
    (
        format!("read_parquet({})", geoparquet_source_literal(entry)),
        entry.geoparquet.clone(),
    )
}

/// Introspects geometry metadata, resolves SRID, and builds a tile-ready `DuckDBSource`.
pub async fn resolve_geoparquet_source(
    source_id: String,
    entry: &GeoParquetEntry,
    pool: DuckDBPool,
    cache: CachePolicy,
) -> DuckDbSourceResult<BoxedSource> {
    let (from_expr, source_label) = geoparquet_from_expr(entry);
    let mut introspection = introspect(&pool, &from_expr, &source_label, &entry.layer).await?;
    introspection.covering = query_covering(
        &pool,
        &geoparquet_source_literal(entry),
        &introspection.geometry_column,
        &source_label,
    )
    .await;
    debug!(
        source.id = %source_id,
        geometry_column = %introspection.geometry_column,
        srid = introspection.srid.get(),
        "Resolved GeoParquet introspection"
    );

    let auto_bounds = entry.settings.auto_bounds.unwrap_or(BoundsCalcType::Quick);
    let bounds = bounds_with_auto(
        &pool,
        &from_expr,
        &source_label,
        &introspection.geometry_column,
        introspection.srid.get(),
        auto_bounds,
    )
    .await?;

    let layer_id = entry.layer_id.as_deref().unwrap_or(&source_id);
    let sql_query = build_mvt_sql(&introspection, &entry.layer, layer_id, &from_expr);
    let tilejson = build_tilejson(
        &introspection,
        &entry.layer,
        layer_id,
        &source_id,
        format!("GeoParquet ({source_label})"),
        bounds,
    );
    let source = DuckDBSource::new(
        source_id,
        DuckDBSqlInfo::new(sql_query, false, "z, x, y".to_owned()),
        tilejson,
        pool,
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
        cache.zoom(),
    );

    Ok(Box::new(source))
}

#[cfg(test)]
#[cfg(feature = "unstable-duckdb")]
mod tests {
    use std::path::PathBuf;

    use duckdb::named_params;
    use martin_core::tiles::duckdb::DuckDBPool;

    use super::*;
    use crate::config::file::tiles::duckdb::resolver::introspect::LayerIntrospection;
    use crate::config::file::tiles::duckdb::sources::MvtLayerOptions;

    const FIXTURE: &str = "../tests/fixtures/duckdb/geoparquet_covering.parquet";

    fn fixture_entry() -> GeoParquetEntry {
        let mut entry = GeoParquetEntry {
            geoparquet: FIXTURE.to_owned(),
            layer: MvtLayerOptions {
                srid: Some(4326),
                ..MvtLayerOptions::default()
            },
            ..GeoParquetEntry::default()
        };
        entry.finalize().expect("finalize the covering fixture");
        entry
    }

    fn fixture_pool() -> DuckDBPool {
        DuckDBPool::new_local_geoparquet(
            "covering".to_owned(),
            PathBuf::from(FIXTURE),
            1,
            None,
            None,
        )
        .expect("local GeoParquet pool")
    }

    async fn fixture_introspection(pool: &DuckDBPool) -> LayerIntrospection {
        let entry = fixture_entry();
        let (from_expr, source_label) = geoparquet_from_expr(&entry);
        let mut introspection = introspect(pool, &from_expr, &source_label, &entry.layer)
            .await
            .expect("introspect the covering fixture");
        introspection.covering = query_covering(
            pool,
            &geoparquet_source_literal(&entry),
            &introspection.geometry_column,
            &source_label,
        )
        .await;
        introspection
    }

    /// The tile one request produced, and the physical operators `DuckDB` used to produce it.
    async fn tile_and_operators(
        pool: &DuckDBPool,
        introspection: &LayerIntrospection,
        z: i16,
        x: i64,
        y: i64,
    ) -> (usize, Vec<String>) {
        // DuckDB derives the profiling format from the file extension, so it has to be .json.
        let profile_dir = tempfile::tempdir().expect("profiling output dir");
        let profile = profile_dir.path().join("profile.json");
        let profile_path = escape_sql_string(&profile.to_string_lossy());
        let entry = fixture_entry();
        let (from_expr, _) = geoparquet_from_expr(&entry);
        let sql = build_mvt_sql(introspection, &entry.layer, "covering", &from_expr);

        let tile = pool
            .generate_tile(move |conn| {
                Ok(conn
                    .execute_batch(&format!(
                        "SET enable_profiling='json';\
                         SET custom_profiling_settings='{{\"OPERATOR_TYPE\":\"true\"}}';\
                         SET profiling_output={profile_path};"
                    ))
                    .and_then(|()| {
                        conn.prepare(&sql)?
                            .query_one(named_params! { "z": z, "x": x, "y": y }, |row| {
                                row.get::<_, Option<Vec<u8>>>(0)
                            })
                    }))
            })
            .await
            .expect("pool")
            .expect("tile query");

        let profile = std::fs::read_to_string(&profile).expect("profiling output");
        let profile = serde_json::from_str::<serde_json::Value>(&profile).expect("json profile");
        let mut operators = Vec::new();
        collect_operators(&profile, &mut operators);

        (tile.map_or(0, |tile| tile.len()), operators)
    }

    fn collect_operators(node: &serde_json::Value, out: &mut Vec<String>) {
        if let Some(operator) = node["operator_type"].as_str() {
            out.push(operator.to_owned());
        }
        for child in node["children"].as_array().into_iter().flatten() {
            collect_operators(child, out);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn the_fixture_declares_a_covering() {
        let pool = fixture_pool();
        let introspection = fixture_introspection(&pool).await;
        assert!(
            introspection.covering.is_some(),
            "the pruning tests below are meaningless without one"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_tile_no_feature_reaches_is_answered_from_parquet_statistics_alone() {
        let pool = fixture_pool();
        let introspection = fixture_introspection(&pool).await;
        let unpruned = LayerIntrospection {
            covering: None,
            ..introspection.clone()
        };

        // z2/x3/y1 is the north-eastern quarter of the world; the fixture is entirely west of it.
        let (tile, operators) = tile_and_operators(&pool, &introspection, 2, 3, 1).await;
        let (control_tile, control_operators) = tile_and_operators(&pool, &unpruned, 2, 3, 1).await;

        assert_eq!(
            tile, control_tile,
            "pruning must not change the tile it produces"
        );
        assert!(
            operators.contains(&"EMPTY_RESULT".to_owned()),
            "the covering predicate did not reach the Parquet reader: {operators:?}"
        );
        assert!(
            control_operators.contains(&"TABLE_SCAN".to_owned()),
            "without the covering predicate this tile must scan, or the assertion above proves nothing: {control_operators:?}"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn pruning_does_not_change_a_tile_that_has_features() {
        let pool = fixture_pool();
        let introspection = fixture_introspection(&pool).await;
        let unpruned = LayerIntrospection {
            covering: None,
            ..introspection.clone()
        };

        // z2/x0/y1 is the north-western quarter of the world, where the fixture's points are.
        let (tile, _) = tile_and_operators(&pool, &introspection, 2, 0, 1).await;
        let (control_tile, _) = tile_and_operators(&pool, &unpruned, 2, 0, 1).await;

        assert!(tile > 0, "the tile should carry features");
        assert_eq!(
            tile, control_tile,
            "pruning must not change the tile it produces"
        );
    }
}
