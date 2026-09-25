use std::path::Path;

use futures::future::join_all;
use martin_core::tiles::duckdb::{DuckDBPool, DuckDBSource, DuckDBSqlInfo};
use martin_core::tiles::{BackendSource, BoxedSource};
use martin_tile_utils::{Encoding, Format, TileInfo};
use tilejson::tilejson;
use tracing::{debug, info};

use crate::config::args::BoundsCalcType;
use crate::config::file::tiles::duckdb::resolver::bounds::bounds_with_auto;
use crate::config::file::tiles::duckdb::resolver::database::discover::{
    discover_macros, discover_tables,
};
use crate::config::file::tiles::duckdb::resolver::errors::DuckDbSourceResult;
use crate::config::file::tiles::duckdb::resolver::introspect::introspect;
use crate::config::file::tiles::duckdb::resolver::metadata::build_tilejson;
use crate::config::file::tiles::duckdb::resolver::sql::build_mvt_sql;
use crate::config::file::tiles::duckdb::sources::{
    DuckDbDatabaseEntry, DuckDbMacroEntry, DuckDbTableEntry,
};
use crate::config::file::tiles::duckdb::sql_utils::escape_identifier;
use crate::config::file::{CachePolicy, TileSourceWarning};
use crate::config::primitives::IdResolver;

/// Opens the database file once and resolves every configured and discovered table and macro
/// of it.
pub async fn resolve_database_entry(
    entry: &DuckDbDatabaseEntry,
    id_resolver: &IdResolver,
    default_cache: CachePolicy,
) -> Vec<Result<BoxedSource, TileSourceWarning>> {
    let path = entry
        .path
        .clone()
        .expect("DuckDbDatabaseEntry must be finalized before resolve");
    let database_label = entry.database.to_string_lossy().into_owned();
    let pool_size = entry
        .settings
        .pool_size
        .expect("pool_size must be set by DuckDbConfig::finalize")
        .get();
    let auto_bounds = entry.settings.auto_bounds.unwrap_or(BoundsCalcType::Quick);
    let entry_warning = |error: String| TileSourceWarning::SourceError {
        source_id: id_resolver.resolve(entry.stem(), path.to_string_lossy().into_owned()),
        error,
    };

    let pool = match DuckDBPool::new_database_file(
        database_label.clone(),
        path.clone(),
        pool_size,
        entry.settings.threads,
        entry.settings.memory_limit_mb,
    ) {
        Ok(pool) => pool,
        Err(error) => return vec![Err(entry_warning(error.to_string()))],
    };

    let tables = match discover_tables_for(entry, &pool, &database_label).await {
        Ok(tables) => tables,
        Err(error) => return vec![Err(entry_warning(error.to_string()))],
    };
    let mut resolved = resolve_tables(
        tables,
        &path,
        &database_label,
        &pool,
        id_resolver,
        auto_bounds,
        default_cache,
    )
    .await;

    let macros = match discover_macros_for(entry, &pool, &database_label).await {
        Ok(macros) => macros,
        Err(error) => {
            resolved.push(Err(entry_warning(error.to_string())));
            return resolved;
        }
    };
    for (id, r#macro) in macros {
        let source_id = id_resolver.resolve(
            &id,
            format!(
                "{}:{}.{}()",
                path.to_string_lossy(),
                r#macro.schema(),
                r#macro.r#macro
            ),
        );
        info!(source.id = %source_id, "Configured DuckDB macro source");
        resolved.push(Ok(resolve_macro_source(
            source_id,
            &database_label,
            &r#macro,
            pool.clone(),
            default_cache,
        )));
    }
    resolved
}

/// The configured tables of the entry, followed by the auto-published ones.
async fn discover_tables_for(
    entry: &DuckDbDatabaseEntry,
    pool: &DuckDBPool,
    database_label: &str,
) -> DuckDbSourceResult<Vec<(String, DuckDbTableEntry)>> {
    let mut tables = entry
        .tables
        .iter()
        .flatten()
        .map(|(id, table)| (id.clone(), table.clone()))
        .collect::<Vec<_>>();
    if let Some(discovery) = entry.table_discovery() {
        tables.extend(
            discover_tables(pool, database_label, &discovery)
                .await?
                .into_iter()
                .map(|table| (table.source_id, table.entry)),
        );
    }
    Ok(tables)
}

/// The configured macros of the entry, followed by the auto-published ones.
async fn discover_macros_for(
    entry: &DuckDbDatabaseEntry,
    pool: &DuckDBPool,
    database_label: &str,
) -> DuckDbSourceResult<Vec<(String, DuckDbMacroEntry)>> {
    let mut macros = entry
        .macros
        .iter()
        .flatten()
        .map(|(id, r#macro)| (id.clone(), r#macro.clone()))
        .collect::<Vec<_>>();
    if let Some(discovery) = entry.macro_discovery() {
        macros.extend(
            discover_macros(pool, database_label, &discovery)
                .await?
                .into_iter()
                .map(|r#macro| (r#macro.source_id, r#macro.entry)),
        );
    }
    Ok(macros)
}

async fn resolve_tables(
    tables: Vec<(String, DuckDbTableEntry)>,
    path: &Path,
    database_label: &str,
    pool: &DuckDBPool,
    id_resolver: &IdResolver,
    auto_bounds: BoundsCalcType,
    default_cache: CachePolicy,
) -> Vec<Result<BoxedSource, TileSourceWarning>> {
    let pending = tables.into_iter().map(|(id, table)| {
        let source_id = id_resolver.resolve(
            &id,
            format!(
                "{}:{}.{}.{}",
                path.to_string_lossy(),
                table.schema(),
                table.table,
                table.layer.geometry_column.as_deref().unwrap_or_default()
            ),
        );
        let pool = pool.clone();
        let database_label = database_label.to_owned();
        async move {
            match resolve_table_source(
                source_id.clone(),
                &database_label,
                &table,
                pool,
                auto_bounds,
                default_cache,
            )
            .await
            {
                Ok(source) => {
                    info!(source.id = %source_id, "Configured DuckDB table source");
                    Ok(source)
                }
                Err(error) => Err(TileSourceWarning::SourceError {
                    source_id,
                    error: error.to_string(),
                }),
            }
        }
    });
    join_all(pending).await
}

/// Builds a `DuckDBSource` that answers each tile with the first column of the macro's first row.
#[must_use]
pub fn resolve_macro_source(
    source_id: String,
    database_label: &str,
    entry: &DuckDbMacroEntry,
    pool: DuckDBPool,
    cache: CachePolicy,
) -> BoxedSource {
    let sql_query = format!(
        "SELECT * FROM {}.{}($z::INTEGER, $x::INTEGER, $y::INTEGER) LIMIT 1",
        escape_identifier(entry.schema()),
        escape_identifier(&entry.r#macro)
    );
    let mut tilejson = tilejson! {
        tiles: vec![],
        name: source_id.clone(),
        description: format!("DuckDB macro {}.{} ({database_label})", entry.schema(), entry.r#macro),
    };
    tilejson.minzoom = entry.minzoom;
    tilejson.maxzoom = entry.maxzoom;
    tilejson.bounds = entry.bounds;
    BackendSource::DuckDb(DuckDBSource::new(
        source_id,
        DuckDBSqlInfo::new(sql_query, false, "z, x, y".to_owned()),
        tilejson,
        pool,
        TileInfo::new(Format::Mvt, Encoding::Uncompressed),
        cache.zoom(),
    ))
    .boxed()
}

/// Introspects one table of a database file and builds a tile-ready `DuckDBSource` for it.
pub async fn resolve_table_source(
    source_id: String,
    database_label: &str,
    entry: &DuckDbTableEntry,
    pool: DuckDBPool,
    auto_bounds: BoundsCalcType,
    cache: CachePolicy,
) -> DuckDbSourceResult<BoxedSource> {
    let relation = format!("{}.{}", entry.schema(), entry.table);
    let from_expr = format!(
        "{}.{}",
        escape_identifier(entry.schema()),
        escape_identifier(&entry.table)
    );
    let source_label = format!("{relation} ({database_label})");
    let introspection = introspect(&pool, &from_expr, &source_label, &entry.layer).await?;
    debug!(
        source.id = %source_id,
        geometry_column = %introspection.geometry_column,
        srid = introspection.srid.get(),
        "Resolved DuckDB table introspection"
    );

    let bounds = bounds_with_auto(
        &pool,
        &from_expr,
        &source_label,
        &introspection.geometry_column,
        introspection.srid.get(),
        auto_bounds,
    )
    .await?;

    let sql_query = build_mvt_sql(&introspection, &entry.layer, &source_id, &from_expr);
    let tilejson = build_tilejson(
        &introspection,
        &entry.layer,
        &source_id,
        &source_id,
        format!("DuckDB table {source_label}"),
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

    Ok(BackendSource::DuckDb(source).boxed())
}
