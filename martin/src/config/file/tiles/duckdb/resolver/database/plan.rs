//! Decides what a `DuckDB` database file auto-publishes from its config and a catalog snapshot.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::num::NonZeroU32;

use crate::config::file::tiles::duckdb::resolver::mvt_types::mvt_property_type;
use crate::config::file::tiles::duckdb::sources::{
    DuckDbCfgPublish, DuckDbDatabaseEntry, DuckDbMacroEntry, DuckDbTableEntry, MvtLayerOptions,
};
use crate::config::primitives::IdFormat;
use crate::config::primitives::OptBoolObj::{self, Bool, NoValue, Object};

/// What one `DuckDB` database file auto-publishes, decoded from its config.
#[derive(Debug)]
pub struct AutoPublish {
    tables: Option<TableRules>,
    macros: Option<MacroRules>,
}

/// How catalog tables are auto-published.
#[derive(Debug)]
struct TableRules {
    schemas: Option<HashSet<String>>,
    id_format: String,
    id_columns: Vec<String>,
    clip_geom: Option<bool>,
    buffer: Option<u32>,
    extent: Option<NonZeroU32>,
}

/// How catalog table macros are auto-published.
#[derive(Debug)]
struct MacroRules {
    schemas: Option<HashSet<String>>,
    id_format: String,
}

/// The catalog queries a discovery needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub struct CatalogReads {
    /// Whether to run the column query.
    pub tables: bool,
    /// Whether to run the table macro query.
    pub macros: bool,
}

/// A snapshot of the catalog.
#[derive(Debug, Default)]
pub struct Catalog {
    /// Column types of the non-internal tables and views, by schema, table and column.
    pub tables: BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>,
    /// The non-internal `(z, x, y)` table macros, by schema.
    pub macros: BTreeMap<String, BTreeSet<String>>,
}

/// The configured sources that resolved, which auto-publishing leaves out.
///
/// Names are kept lowercased, as `DuckDB` identifiers are case-insensitive.
#[derive(Debug, Default)]
pub struct Merged {
    tables: HashSet<(String, String, String)>,
    macros: HashSet<(String, String)>,
}

impl Merged {
    /// Records a configured table that resolved with this geometry column.
    pub fn table(&mut self, config: &DuckDbTableEntry, geometry_column: &str) {
        self.tables
            .insert((lower(config.schema()), lower(&config.table), lower(geometry_column)));
    }

    /// Records a configured table macro.
    pub fn r#macro(&mut self, config: &DuckDbMacroEntry) {
        self.macros.insert((lower(config.schema()), lower(&config.r#macro)));
    }
}

/// The sources to auto-publish under the ids they ask for, and what was decided on the way.
#[derive(Debug, Default)]
pub struct Plan {
    /// Tables to publish, in catalog order.
    pub tables: Vec<(String, DuckDbTableEntry)>,
    /// Table macros to publish, in catalog order.
    pub macros: Vec<(String, DuckDbMacroEntry)>,
    /// Everything worth logging, in the order it was decided.
    pub decisions: Vec<Decision>,
}

/// One thing the plan decided.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub enum Decision {
    /// The search for a table's feature id column found something worth reporting.
    IdColumn {
        schema: String,
        table: String,
        outcome: IdColumnOutcome,
    },
}

/// What the search for a feature id column reports.
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub enum IdColumnOutcome {
    /// The column is not an integer, so it is not used.
    NotInteger { column: String, data_type: String },
    /// No column was used.
    NoneFound { searched: Vec<String> },
}

impl AutoPublish {
    /// Decodes the auto-publish settings of one database entry.
    #[must_use]
    pub fn new(entry: &DuckDbDatabaseEntry) -> Self {
        let publish = match &entry.auto_publish {
            Object(publish) => Some(publish),
            NoValue | Bool(_) => None,
        };
        let (tables_on, macros_on) = match &entry.auto_publish {
            Object(p) => (is_on(&p.tables, &p.macros), is_on(&p.macros, &p.tables)),
            Bool(on) => (*on, *on),
            NoValue => (false, false),
        };

        let table_cfg = match publish.map(|p| &p.tables) {
            Some(Object(cfg)) => Some(cfg),
            _ => None,
        };
        let tables = tables_on.then(|| TableRules {
            schemas: schemas(publish, table_cfg.map_or(&[], |cfg| cfg.from_schemas.as_slice())),
            id_format: table_cfg
                .and_then(|cfg| cfg.source_id_format.as_ref())
                .map_or("{table}", IdFormat::as_str)
                .to_owned(),
            id_columns: table_cfg.map(|cfg| cfg.id_columns.clone()).unwrap_or_default(),
            clip_geom: table_cfg.and_then(|cfg| cfg.clip_geom),
            buffer: table_cfg.and_then(|cfg| cfg.buffer),
            extent: table_cfg.and_then(|cfg| cfg.extent),
        });

        let macro_cfg = match publish.map(|p| &p.macros) {
            Some(Object(cfg)) => Some(cfg),
            _ => None,
        };
        let macros = macros_on.then(|| MacroRules {
            schemas: schemas(publish, macro_cfg.map_or(&[], |cfg| cfg.from_schemas.as_slice())),
            id_format: macro_cfg
                .and_then(|cfg| cfg.source_id_format.as_ref())
                .map_or("{macro}", IdFormat::as_str)
                .to_owned(),
        });

        Self { tables, macros }
    }

    /// The catalog queries that auto-publishing needs.
    #[must_use]
    pub fn reads(&self) -> CatalogReads {
        CatalogReads {
            tables: self.tables.is_some(),
            macros: self.macros.is_some(),
        }
    }

    /// The tables and table macros to auto-publish from `catalog`.
    #[must_use]
    pub fn plan(&self, catalog: &Catalog, merged: &Merged) -> Plan {
        let mut plan = Plan::default();
        if let Some(rules) = &self.tables {
            rules.plan(&catalog.tables, merged, &mut plan);
        }
        if let Some(rules) = &self.macros {
            rules.plan(&catalog.macros, merged, &mut plan);
        }
        plan
    }
}

impl TableRules {
    fn plan(
        &self,
        tables: &BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>,
        merged: &Merged,
        plan: &mut Plan,
    ) {
        for (schema, in_schema) in scanned(tables, self.schemas.as_ref()) {
            for (table, columns) in in_schema {
                for (column, data_type) in columns {
                    if !data_type.to_ascii_uppercase().starts_with("GEOMETRY")
                        || merged.tables.contains(&(lower(schema), lower(table), lower(column)))
                    {
                        continue;
                    }
                    let id = self
                        .id_format
                        .replace("{schema}", schema)
                        .replace("{table}", table)
                        .replace("{column}", column);
                    let id_column = self.id_column(schema, table, columns, &mut plan.decisions);
                    plan.tables.push((
                        id,
                        DuckDbTableEntry {
                            schema: Some(schema.clone()),
                            table: table.clone(),
                            layer: MvtLayerOptions {
                                id_column,
                                geometry_column: Some(column.clone()),
                                extent: self.extent,
                                buffer: self.buffer,
                                clip_geom: self.clip_geom,
                                ..MvtLayerOptions::default()
                            },
                            ..DuckDbTableEntry::default()
                        },
                    ));
                }
            }
        }
    }

    /// The first of the wanted id columns that the table has as an integer column.
    fn id_column(
        &self,
        schema: &str,
        table: &str,
        columns: &BTreeMap<String, String>,
        decisions: &mut Vec<Decision>,
    ) -> Option<String> {
        if self.id_columns.is_empty() {
            return None;
        }
        let mut report = |outcome| {
            decisions.push(Decision::IdColumn {
                schema: schema.to_owned(),
                table: table.to_owned(),
                outcome,
            });
        };
        for wanted in &self.id_columns {
            let Some((column, data_type)) = columns.iter().find(|(column, _)| lower(column) == lower(wanted)) else {
                continue;
            };
            if let Some("INTEGER" | "BIGINT") = mvt_property_type(data_type) {
                return Some(column.clone());
            }
            report(IdColumnOutcome::NotInteger {
                column: column.clone(),
                data_type: data_type.clone(),
            });
        }
        report(IdColumnOutcome::NoneFound {
            searched: self.id_columns.clone(),
        });
        None
    }
}

impl MacroRules {
    fn plan(&self, macros: &BTreeMap<String, BTreeSet<String>>, merged: &Merged, plan: &mut Plan) {
        for (schema, names) in scanned(macros, self.schemas.as_ref()) {
            for name in names {
                if merged.macros.contains(&(lower(schema), lower(name))) {
                    continue;
                }
                let id = self.id_format.replace("{schema}", schema).replace("{macro}", name);
                plan.macros.push((
                    id,
                    DuckDbMacroEntry {
                        schema: Some(schema.clone()),
                        r#macro: name.clone(),
                        ..DuckDbMacroEntry::default()
                    },
                ));
            }
        }
    }
}

/// Whether one source kind is auto-published, where naming only the other kind turns this one off.
const fn is_on<T, U>(this: &OptBoolObj<T>, other: &OptBoolObj<U>) -> bool {
    match this {
        NoValue => matches!(other, NoValue | Bool(false)),
        Object(_) => true,
        Bool(on) => *on,
    }
}

/// The outer and inner `from_schemas` together and lowercased, or `None` when neither names any.
fn schemas(publish: Option<&DuckDbCfgPublish>, inner: &[String]) -> Option<HashSet<String>> {
    let outer = publish.map_or(&[][..], |p| p.from_schemas.as_slice());
    let schemas: HashSet<String> = outer.iter().chain(inner).map(|schema| lower(schema)).collect();
    (!schemas.is_empty()).then_some(schemas)
}

/// The catalog schemas that `wanted` names ignoring case, or all of them when it is `None`.
fn scanned<'a, T>(catalog: &'a BTreeMap<String, T>, wanted: Option<&HashSet<String>>) -> Vec<(&'a String, &'a T)> {
    catalog
        .iter()
        .filter(|(schema, _)| wanted.is_none_or(|wanted| wanted.contains(&lower(schema))))
        .collect()
}

/// The identifier as `DuckDB` compares it.
fn lower(name: &str) -> String {
    name.to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fixture {
        config: DuckDbDatabaseEntry,
        #[serde(default)]
        catalog: FixtureCatalog,
    }

    #[derive(Default, Deserialize)]
    #[serde(default, deny_unknown_fields)]
    struct FixtureCatalog {
        columns: Vec<FixtureColumn>,
        macros: Vec<FixtureMacro>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FixtureColumn {
        schema: String,
        table: String,
        column: String,
        data_type: String,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FixtureMacro {
        schema: String,
        r#macro: String,
    }

    #[derive(Serialize)]
    struct Snapshot {
        reads: CatalogReads,
        tables: Vec<Published<DuckDbTableEntry>>,
        macros: Vec<Published<DuckDbMacroEntry>>,
        decisions: Vec<Decision>,
    }

    #[derive(Serialize)]
    struct Published<T> {
        id: String,
        #[serde(flatten)]
        entry: T,
    }

    #[test]
    fn plan_matches_each_fixture() {
        insta::glob!(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/duckdb_auto_publish"),
            "*.yaml",
            |path| {
                let text = fs::read_to_string(path).expect("read fixture");
                let fixture: Fixture = serde_saphyr::from_str(&text).expect("parse fixture");

                let mut catalog = Catalog::default();
                for column in fixture.catalog.columns {
                    catalog
                        .tables
                        .entry(column.schema)
                        .or_default()
                        .entry(column.table)
                        .or_default()
                        .insert(column.column, column.data_type);
                }
                for found in fixture.catalog.macros {
                    catalog.macros.entry(found.schema).or_default().insert(found.r#macro);
                }

                let mut merged = Merged::default();
                for (_, table) in fixture.config.tables.iter().flatten() {
                    let geometry_column = table
                        .layer
                        .geometry_column
                        .as_deref()
                        .expect("fixture tables name their geometry column");
                    merged.table(table, geometry_column);
                }
                for (_, configured) in fixture.config.macros.iter().flatten() {
                    merged.r#macro(configured);
                }

                let auto = AutoPublish::new(&fixture.config);
                let plan = auto.plan(&catalog, &merged);
                let snapshot = Snapshot {
                    reads: auto.reads(),
                    tables: plan
                        .tables
                        .into_iter()
                        .map(|(id, entry)| Published { id, entry })
                        .collect(),
                    macros: plan
                        .macros
                        .into_iter()
                        .map(|(id, entry)| Published { id, entry })
                        .collect(),
                    decisions: plan.decisions,
                };
                insta::assert_yaml_snapshot!(snapshot);
            }
        );
    }
}
