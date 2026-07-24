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
    let (from_expr, source_label) = geoparquet_from_expr(entry)?;
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
        DuckDBSqlInfo::new(sql_query, false, "z, x, y".to_string()),
        tilejson,
        pool,
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
        cache.zoom(),
    );

    Ok(Box::new(source))
}
