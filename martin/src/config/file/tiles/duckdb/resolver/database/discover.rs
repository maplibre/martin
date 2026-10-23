use martin_core::tiles::duckdb::DuckDBPool;

use crate::config::file::tiles::duckdb::resolver::database::plan::{Catalog, CatalogReads};
use crate::config::file::tiles::duckdb::resolver::errors::{DuckDbSourceError, DuckDbSourceResult};

/// Reads the parts of the catalog that `reads` asks for.
pub(crate) async fn read_catalog(
    pool: &DuckDBPool,
    database_label: &str,
    reads: CatalogReads,
) -> DuckDbSourceResult<Catalog> {
    let mut catalog = Catalog::default();
    if reads.tables {
        for (schema, table, column, data_type) in read_columns(pool, database_label).await? {
            catalog
                .tables
                .entry(schema)
                .or_default()
                .entry(table)
                .or_default()
                .insert(column, data_type);
        }
    }
    if reads.macros {
        for (schema, name) in read_macros(pool, database_label).await? {
            catalog.macros.entry(schema).or_default().insert(name);
        }
    }
    Ok(catalog)
}

/// Every column of the non-internal tables and views, as `(schema, table, column, type)`.
async fn read_columns(
    pool: &DuckDBPool,
    database_label: &str,
) -> DuckDbSourceResult<Vec<(String, String, String, String)>> {
    let query = "SELECT schema_name, table_name, column_name, data_type \
                 FROM duckdb_columns() \
                 WHERE NOT internal \
                 ORDER BY schema_name, table_name, column_index";
    let label = database_label.to_owned();
    pool.generate_tile(move |conn| {
        Ok(conn.prepare(query).and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.collect::<Result<Vec<_>, _>>()
        }))
    })
    .await?
    .map_err(|source| DuckDbSourceError::introspection_query(&source, label, "discover", query.to_owned()))
}

/// Every non-internal table macro whose parameters are exactly `(z, x, y)`, as `(schema, macro)`.
async fn read_macros(pool: &DuckDBPool, database_label: &str) -> DuckDbSourceResult<Vec<(String, String)>> {
    let query = "SELECT schema_name, function_name \
                 FROM duckdb_functions() \
                 WHERE NOT internal \
                   AND function_type = 'table_macro' \
                   AND list_transform(parameters, p -> lower(p)) = ['z', 'x', 'y'] \
                 ORDER BY schema_name, function_name";
    let label = database_label.to_owned();
    pool.generate_tile(move |conn| {
        Ok(conn.prepare(query).and_then(|mut stmt| {
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
            rows.collect::<Result<Vec<_>, _>>()
        }))
    })
    .await?
    .map_err(|source| DuckDbSourceError::introspection_query(&source, label, "discover", query.to_owned()))
}
