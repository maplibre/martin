use martin_core::tiles::BoxedSource;
use martin_core::tiles::duckdb::{DuckDBPool, DuckDBSource, DuckDBSqlInfo};
use martin_tile_utils::{Encoding, Format, TileInfo};
use tracing::debug;

use super::introspect::{geoparquet_from_expr, introspect};
use super::metadata::build_tilejson;
use super::sql::build_mvt_sql;
use crate::config::args::BoundsCalcType;
use crate::config::file::CachePolicy;
use crate::config::file::tiles::duckdb::resolver::bounds::bounds_with_auto;
use crate::config::file::tiles::duckdb::resolver::errors::GeoparquetResult;
use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;

/// Introspects geometry metadata, resolves SRID, and builds a tile-ready `DuckDBSource`.
pub async fn resolve_geoparquet_source(
    source_id: String,
    entry: &GeoParquetEntry,
    pool: DuckDBPool,
    cache: CachePolicy,
) -> GeoparquetResult<BoxedSource> {
    let (from_expr, source_label) = geoparquet_from_expr(entry);
    let introspection = introspect(&pool, &from_expr, &source_label, entry).await?;
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

    let sql_query = build_mvt_sql(&introspection, entry, &source_id, &from_expr);
    let tilejson = build_tilejson(&introspection, entry, &source_id, &source_label, bounds);
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
    use crate::config::file::tiles::duckdb::resolver::geoparquet::introspect::GeoParquetIntrospection;
    use crate::config::file::tiles::duckdb::sql_utils::escape_sql_string;

    const FIXTURE: &str = "../tests/fixtures/duckdb/geoparquet_covering.parquet";

    fn fixture_entry() -> GeoParquetEntry {
        let mut entry = GeoParquetEntry {
            geoparquet: FIXTURE.to_owned(),
            srid: Some(4326),
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

    async fn fixture_introspection(pool: &DuckDBPool) -> GeoParquetIntrospection {
        let entry = fixture_entry();
        let (from_expr, source_label) = geoparquet_from_expr(&entry);
        introspect(pool, &from_expr, &source_label, &entry)
            .await
            .expect("introspect the covering fixture")
    }

    /// The tile one request produced, and the physical operators `DuckDB` used to produce it.
    ///
    /// The operators are the point: when the covering predicate reaches the Parquet reader,
    /// `DuckDB` resolves it against the file's own statistics and a tile that matches nothing
    /// becomes an `EMPTY_RESULT` with no scan at all. Without pushdown the same tile has to
    /// scan the file and filter row by row, which is the 5-minute-tile behaviour this guards.
    async fn tile_and_operators(
        pool: &DuckDBPool,
        introspection: &GeoParquetIntrospection,
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
        let sql = build_mvt_sql(introspection, &entry, "covering", &from_expr);

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
        let unpruned = GeoParquetIntrospection {
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
        let unpruned = GeoParquetIntrospection {
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
