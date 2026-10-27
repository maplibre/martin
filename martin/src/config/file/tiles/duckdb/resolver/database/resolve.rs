use futures::future::join_all;
use martin_core::tiles::duckdb::{DuckDBPool, DuckDBSource, DuckDBSqlInfo};
use martin_core::tiles::{BackendSource, BoxedSource};
use martin_tile_utils::{Encoding, Format, TileInfo};
use tilejson::{Bounds, tilejson};
use tracing::{debug, info};

use crate::config::args::BoundsCalcType;
use crate::config::file::tiles::duckdb::resolver::bounds::bounds_with_auto;
use crate::config::file::tiles::duckdb::resolver::database::discover::read_catalog;
use crate::config::file::tiles::duckdb::resolver::database::plan::{
    AutoPublish, Catalog, Decision, IdColumnOutcome, Merged,
};
use crate::config::file::tiles::duckdb::resolver::errors::DuckDbSourceResult;
use crate::config::file::tiles::duckdb::resolver::introspect::{LayerIntrospection, introspect};
use crate::config::file::tiles::duckdb::resolver::metadata::build_tilejson;
use crate::config::file::tiles::duckdb::resolver::sql::{build_mvt_sql, parse_cql2_filter};
use crate::config::file::tiles::duckdb::sources::{DuckDbDatabaseEntry, DuckDbMacroEntry, DuckDbTableEntry};
use crate::config::file::tiles::duckdb::sql_utils::escape_identifier;
use crate::config::file::{CachePolicy, TileSourceWarning};
use crate::config::primitives::IdResolver;

/// The tables and macros of one database file, introspected but without source ids yet.
#[derive(Debug)]
pub struct PreparedDatabase {
    stem: String,
    path: String,
    database_label: String,
    pool: Option<DuckDBPool>,
    errors: Vec<String>,
    tables: Vec<(String, DuckDbTableEntry, DuckDbSourceResult<IntrospectedTable>)>,
    macros: Vec<(String, DuckDbMacroEntry)>,
}

/// Opens the database file once and introspects every configured and auto-published table of it.
pub async fn prepare_database_entry(entry: &DuckDbDatabaseEntry) -> PreparedDatabase {
    let path = entry
        .path
        .clone()
        .expect("DuckDbDatabaseEntry must be finalized before resolve");
    let database_label = entry.database.to_string_lossy().into_owned();
    let mut prepared = PreparedDatabase {
        stem: entry.stem().to_owned(),
        path: path.to_string_lossy().into_owned(),
        database_label: database_label.clone(),
        pool: None,
        errors: Vec::new(),
        tables: Vec::new(),
        macros: Vec::new(),
    };
    let pool_size = entry
        .settings
        .pool_size
        .expect("pool_size must be set by DuckDbConfig::finalize")
        .get();
    let auto_bounds = entry.settings.auto_bounds.unwrap_or(BoundsCalcType::Quick);

    let pool = match DuckDBPool::new_database_file(
        database_label.clone(),
        path,
        pool_size,
        entry.settings.threads,
        entry.settings.memory_limit_mb,
    ) {
        Ok(pool) => pool,
        Err(error) => {
            prepared.errors.push(error.to_string());
            return prepared;
        }
    };

    let configured_tables = entry
        .tables
        .iter()
        .flatten()
        .map(|(id, table)| (id.clone(), table.clone()));
    prepared.tables = introspect_tables(configured_tables, &database_label, &pool, auto_bounds).await;
    prepared.macros = entry
        .macros
        .iter()
        .flatten()
        .map(|(id, r#macro)| (id.clone(), r#macro.clone()))
        .collect();

    let mut merged = Merged::default();
    for (_, table, introspected) in &prepared.tables {
        if let Ok(introspected) = introspected {
            merged.table(table, &introspected.introspection.geometry_column);
        }
    }
    for (_, r#macro) in &prepared.macros {
        merged.r#macro(r#macro);
    }

    let auto_publish = AutoPublish::new(entry);
    let catalog = match read_catalog(&pool, &database_label, auto_publish.reads()).await {
        Ok(catalog) => catalog,
        Err(error) => {
            prepared.errors.push(error.to_string());
            Catalog::default()
        }
    };
    let plan = auto_publish.plan(&catalog, &merged);
    for decision in &plan.decisions {
        log_decision(decision);
    }
    let auto_tables = introspect_tables(plan.tables, &database_label, &pool, auto_bounds).await;
    prepared.tables.extend(auto_tables);
    prepared.macros.extend(plan.macros);
    prepared.pool = Some(pool);
    prepared
}

impl PreparedDatabase {
    /// Gives each source its id and builds it, configured tables first, then auto-published
    /// tables, configured macros and auto-published macros.
    pub fn resolve(
        self,
        id_resolver: &IdResolver,
        default_cache: &CachePolicy,
    ) -> Vec<Result<BoxedSource, TileSourceWarning>> {
        let Self {
            stem,
            path,
            database_label,
            pool,
            errors,
            tables,
            macros,
        } = self;
        let mut resolved = errors
            .into_iter()
            .map(|error| {
                Err(TileSourceWarning::SourceError {
                    source_id: id_resolver.resolve(&stem, path.clone()),
                    error,
                })
            })
            .collect::<Vec<_>>();
        let Some(pool) = pool else {
            return resolved;
        };

        for (id, table, introspected) in tables {
            resolved.push(match introspected {
                Ok(introspected) => {
                    let source_id = id_resolver.resolve(
                        &id,
                        format!(
                            "{path}:{}.{}.{}",
                            table.schema(),
                            table.table,
                            introspected.introspection.geometry_column
                        ),
                    );
                    info!(source.id = %source_id, "Configured DuckDB table source");
                    Ok(introspected.build(source_id, &table, pool.clone(), default_cache))
                }
                Err(error) => Err(TileSourceWarning::SourceError {
                    source_id: id,
                    error: error.to_string(),
                }),
            });
        }
        for (id, r#macro) in macros {
            let source_id = id_resolver.resolve(&id, format!("{path}:{}.{}()", r#macro.schema(), r#macro.r#macro));
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
}

/// Introspects the tables in parallel, keeping each next to the id it asks for.
async fn introspect_tables(
    tables: impl IntoIterator<Item = (String, DuckDbTableEntry)>,
    database_label: &str,
    pool: &DuckDBPool,
    auto_bounds: BoundsCalcType,
) -> Vec<(String, DuckDbTableEntry, DuckDbSourceResult<IntrospectedTable>)> {
    let pending = tables.into_iter().map(|(id, table)| async move {
        let introspected = introspect_table(database_label, &table, pool, auto_bounds).await;
        (id, table, introspected)
    });
    join_all(pending).await
}

fn log_decision(decision: &Decision) {
    match decision {
        Decision::IdColumn {
            schema,
            table,
            outcome: IdColumnOutcome::NotInteger { column, data_type },
        } => info!(
            schema,
            table, column, data_type, "Skipping ID column: only integer columns can be MVT feature ids"
        ),
        Decision::IdColumn {
            schema,
            table,
            outcome: IdColumnOutcome::NoneFound { searched },
        } => info!(
            schema,
            table,
            searched = searched.join(", "),
            "No ID column found for table - searched for an integer column"
        ),
    }
}

/// Builds a `DuckDBSource` that answers each tile with the first column of the macro's first row.
#[must_use]
pub fn resolve_macro_source(
    source_id: String,
    database_label: &str,
    entry: &DuckDbMacroEntry,
    pool: DuckDBPool,
    cache: &CachePolicy,
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

/// One table of a database file whose columns, SRID and bounds are known.
#[derive(Debug)]
pub struct IntrospectedTable {
    introspection: LayerIntrospection,
    filter_sql: Option<String>,
    from_expr: String,
    source_label: String,
    bounds: Option<Bounds>,
}

/// Introspects one table of a database file and computes its bounds.
pub async fn introspect_table(
    database_label: &str,
    entry: &DuckDbTableEntry,
    pool: &DuckDBPool,
    auto_bounds: BoundsCalcType,
) -> DuckDbSourceResult<IntrospectedTable> {
    let filter_sql = parse_cql2_filter(entry.layer.filter.as_deref())?;
    let relation = format!("{}.{}", entry.schema(), entry.table);
    let from_expr = format!(
        "{}.{}",
        escape_identifier(entry.schema()),
        escape_identifier(&entry.table)
    );
    let source_label = format!("{relation} ({database_label})");
    let introspection = introspect(pool, &from_expr, &source_label, &entry.layer).await?;
    let bounds = bounds_with_auto(
        pool,
        &from_expr,
        &source_label,
        &introspection.geometry_column,
        introspection.srid.get(),
        auto_bounds,
        filter_sql.as_deref(),
    )
    .await?;
    Ok(IntrospectedTable {
        introspection,
        filter_sql,
        from_expr,
        source_label,
        bounds,
    })
}

impl IntrospectedTable {
    /// Builds a tile-ready `DuckDBSource` for the table under `source_id`.
    #[must_use]
    pub fn build(
        self,
        source_id: String,
        entry: &DuckDbTableEntry,
        pool: DuckDBPool,
        cache: &CachePolicy,
    ) -> BoxedSource {
        debug!(
            source.id = %source_id,
            geometry_column = %self.introspection.geometry_column,
            srid = self.introspection.srid.get(),
            "Resolved DuckDB table introspection"
        );
        let sql_query = build_mvt_sql(
            &self.introspection,
            &entry.layer,
            &source_id,
            &self.from_expr,
            self.filter_sql.as_deref(),
        );
        let tilejson = build_tilejson(
            &self.introspection,
            &entry.layer,
            &source_id,
            &source_id,
            format!("DuckDB table {}", self.source_label),
            self.bounds,
        );
        let source = DuckDBSource::new(
            source_id,
            DuckDBSqlInfo::new(sql_query, false, "z, x, y".to_owned()),
            tilejson,
            pool,
            TileInfo::new(Format::Mvt, Encoding::Uncompressed),
            cache.zoom(),
        );
        BackendSource::DuckDb(source).boxed()
    }
}
