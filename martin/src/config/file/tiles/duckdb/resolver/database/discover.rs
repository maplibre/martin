use std::collections::BTreeMap;

use martin_core::tiles::duckdb::DuckDBPool;
use tracing::info;

use crate::config::file::tiles::duckdb::resolver::errors::{DuckDbSourceError, DuckDbSourceResult};
use crate::config::file::tiles::duckdb::resolver::mvt_types::mvt_property_type;
use crate::config::file::tiles::duckdb::sources::auto_publish::{MacroDiscovery, TableDiscovery};
use crate::config::file::tiles::duckdb::sources::{
    DuckDbMacroEntry, DuckDbTableEntry, MvtLayerOptions,
};

/// One geometry column of a relation found by [`discover_tables`], as a table entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscoveredTable {
    pub source_id: String,
    pub entry: DuckDbTableEntry,
}

/// Lists every geometry column of the non-internal tables and views in the wanted schemas,
/// one candidate source per column.
pub(crate) async fn discover_tables(
    pool: &DuckDBPool,
    database_label: &str,
    discovery: &TableDiscovery,
) -> DuckDbSourceResult<Vec<DiscoveredTable>> {
    let query = "SELECT schema_name, table_name, column_name, data_type \
                 FROM duckdb_columns() \
                 WHERE NOT internal \
                 ORDER BY schema_name, table_name, column_index";
    let label = database_label.to_owned();
    let columns = pool
        .generate_tile(move |conn| {
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
        .map_err(|source| {
            DuckDbSourceError::introspection_query(source, label, "discover", query.to_owned())
        })?;

    let mut relations: BTreeMap<(String, String), BTreeMap<String, String>> = BTreeMap::new();
    for (schema, table, column, data_type) in columns {
        if discovery
            .schemas
            .as_ref()
            .is_some_and(|schemas| !schemas.contains(&schema))
        {
            continue;
        }
        relations
            .entry((schema, table))
            .or_default()
            .insert(column, data_type);
    }

    let mut discovered = Vec::new();
    for ((schema, table), columns) in relations {
        for (column, data_type) in &columns {
            if !data_type.to_ascii_uppercase().starts_with("GEOMETRY") {
                continue;
            }
            let source_id = discovery
                .source_id_format
                .replace("{schema}", &schema)
                .replace("{table}", &table)
                .replace("{column}", column);
            let id_column =
                find_id_column(&columns, discovery.id_columns.as_deref(), &schema, &table);
            discovered.push(DiscoveredTable {
                source_id,
                entry: DuckDbTableEntry {
                    schema: Some(schema.clone()),
                    table: table.clone(),
                    layer: MvtLayerOptions {
                        id_column,
                        geometry_column: Some(column.clone()),
                        extent: discovery.extent,
                        buffer: discovery.buffer,
                        clip_geom: discovery.clip_geom,
                        ..MvtLayerOptions::default()
                    },
                    ..DuckDbTableEntry::default()
                },
            });
        }
    }
    Ok(discovered)
}

/// One `(z, x, y)` table macro found by [`discover_macros`], as a macro entry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DiscoveredMacro {
    pub source_id: String,
    pub entry: DuckDbMacroEntry,
}

/// Lists every non-internal table macro in the wanted schemas whose parameters are exactly
/// `(z, x, y)`.
pub(crate) async fn discover_macros(
    pool: &DuckDBPool,
    database_label: &str,
    discovery: &MacroDiscovery,
) -> DuckDbSourceResult<Vec<DiscoveredMacro>> {
    let query = "SELECT schema_name, function_name \
                 FROM duckdb_functions() \
                 WHERE NOT internal \
                   AND function_type = 'table_macro' \
                   AND list_transform(parameters, p -> lower(p)) = ['z', 'x', 'y'] \
                 ORDER BY schema_name, function_name";
    let label = database_label.to_owned();
    let macros = pool
        .generate_tile(move |conn| {
            Ok(conn.prepare(query).and_then(|mut stmt| {
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                rows.collect::<Result<Vec<_>, _>>()
            }))
        })
        .await?
        .map_err(|source| {
            DuckDbSourceError::introspection_query(source, label, "discover", query.to_owned())
        })?;

    Ok(macros
        .into_iter()
        .filter(|(schema, _)| {
            discovery
                .schemas
                .as_ref()
                .is_none_or(|schemas| schemas.contains(schema))
        })
        .map(|(schema, name)| DiscoveredMacro {
            source_id: discovery
                .source_id_format
                .replace("{schema}", &schema)
                .replace("{macro}", &name),
            entry: DuckDbMacroEntry {
                schema: Some(schema),
                r#macro: name,
                ..DuckDbMacroEntry::default()
            },
        })
        .collect())
}

fn find_id_column(
    columns: &BTreeMap<String, String>,
    id_columns: Option<&[String]>,
    schema: &str,
    table: &str,
) -> Option<String> {
    let id_columns = id_columns?;
    for candidate in id_columns {
        let Some(data_type) = columns.get(candidate) else {
            continue;
        };
        match mvt_property_type(data_type) {
            Some("INTEGER" | "BIGINT") => return Some(candidate.clone()),
            _ => {
                info!(
                    schema,
                    table,
                    column = candidate,
                    data_type,
                    "Skipping ID column: only integer columns can be MVT feature ids"
                );
            }
        }
    }
    info!(
        schema,
        table,
        searched = id_columns.join(", "),
        "No ID column found for table - searched for an integer column"
    );
    None
}
