//! Decides what a `PostgreSQL` connection auto-publishes from its config and a catalog snapshot.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroU32;

use martin_core::tiles::postgres::PostgresSqlInfo;

use crate::config::file::postgres::resolver::{SqlFuncInfoMapMap, SqlTableInfoMapMapMap, function_name};
use crate::config::file::postgres::utils::find_kv_ignore_case;
use crate::config::file::postgres::{
    FunctionInfo, IdFormat, PgTileGrid, PostgresCfgPublish, PostgresConfig, TableInfo,
};
use crate::config::primitives::OptBoolObj::{self, Bool, NoValue, Object};

/// What one `PostgreSQL` connection auto-publishes, decoded from its config.
#[derive(Debug)]
pub struct AutoPublish {
    tables: Option<TableRules>,
    functions: Option<FunctionRules>,
    configured_tables: HashSet<TableKey>,
    configured_functions: HashSet<FunctionKey>,
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

/// How catalog functions are auto-published.
#[derive(Debug)]
struct FunctionRules {
    schemas: Option<HashSet<String>>,
    id_format: String,
}

/// The catalog queries a discovery needs.
#[derive(Debug, PartialEq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub struct CatalogReads {
    /// The table query, or `None` to skip it.
    pub tables: Option<TableReads>,
    /// Whether to run the function query.
    pub functions: bool,
}

/// What the table query reads.
#[derive(Debug, PartialEq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub struct TableReads {
    /// The lowercased schemas to read, or `None` for every schema.
    pub schemas: Option<Vec<String>>,
    /// The lowercased `(schema, table)` pairs to keep, or `None` to keep every table.
    pub only: Option<HashSet<(String, String)>>,
}

/// A snapshot of the catalog.
#[derive(Debug, Default)]
pub struct Catalog {
    /// Tables with a geometry column, by schema, table and geometry column.
    pub tables: SqlTableInfoMapMapMap,
    /// Tile-serving functions, by schema and function.
    pub functions: SqlFuncInfoMapMap,
    /// Whether `PostGIS` can convert each pair that [`AutoPublish::transforms`] asked for.
    pub transforms: HashMap<Transform, bool>,
}

/// A table stored in one SRID served in a grid of another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(test, derive(serde::Serialize, serde::Deserialize))]
pub struct Transform {
    /// The SRID the table is read in.
    pub table: i32,
    /// The SRID of the tile grid.
    pub grid: i32,
}

/// One geometry column of a table.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(test, derive(serde::Serialize))]
pub struct TableKey {
    /// The table's schema.
    pub schema: String,
    /// The table's name.
    pub table: String,
    /// The geometry column's name.
    pub geometry_column: String,
}

impl Display for TableKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.schema, self.table, self.geometry_column)
    }
}

/// One function.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct FunctionKey {
    schema: String,
    function: String,
}

/// The sources to auto-publish under the ids they ask for, and what was decided on the way.
#[derive(Debug, Default)]
pub struct Plan {
    /// Tables to publish, in catalog order.
    pub tables: Vec<(String, TableInfo)>,
    /// Functions to publish, in catalog order.
    pub functions: Vec<(String, FunctionInfo, PostgresSqlInfo)>,
    /// Everything worth logging, in the order it was decided.
    pub decisions: Vec<Decision>,
}

/// One thing the plan decided.
#[derive(Debug, PartialEq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub enum Decision {
    /// Tables are auto-published from these catalog schemas.
    ScannedTables { schemas: Vec<String>, id_format: String },
    /// Functions are auto-published from these catalog schemas.
    ScannedFunctions { schemas: Vec<String>, id_format: String },
    /// A table is not published.
    Skipped { relation: TableKey, reason: SkipReason },
    /// A table without an SRID is read in the default SRID.
    DefaultSrid { relation: TableKey, srid: i32 },
    /// The search for a table's feature id column found something worth reporting.
    IdColumn {
        relation: TableKey,
        outcome: IdColumnOutcome,
    },
}

/// Why a table is not published.
#[derive(Debug, PartialEq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub enum SkipReason {
    /// The table has SRID 0 and there is no default SRID.
    NoSrid,
    /// The table has this SRID, but the grid is simple.
    SridOnSimpleGrid(i32),
    /// `PostGIS` cannot convert the table into the grid's CRS.
    CannotTransform(Transform),
}

/// What the search for a feature id column reports.
#[derive(Debug, PartialEq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub enum IdColumnOutcome {
    /// The column only matched ignoring case.
    CaseInsensitive { requested: String, found: String },
    /// Several columns matched ignoring case, so none is used.
    Ambiguous { requested: String, candidates: Vec<String> },
    /// The column is not an integer, so it is not used.
    NotInteger { column: String, typ: String },
    /// No column was used.
    NoneFound { searched: Vec<String> },
}

impl AutoPublish {
    /// Decodes the auto-publish settings and the configured sources of one connection.
    #[must_use]
    pub fn new(config: &PostgresConfig) -> Self {
        let publish = match &config.auto_publish {
            Object(publish) => Some(publish),
            NoValue | Bool(_) => None,
        };
        let (tables_on, functions_on) = match &config.auto_publish {
            Object(p) => (is_on(&p.tables, &p.functions), is_on(&p.functions, &p.tables)),
            Bool(on) => (*on, *on),
            NoValue => {
                let on = config.tables.is_none() && config.functions.is_none();
                (on, on)
            }
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

        let function_cfg = match publish.map(|p| &p.functions) {
            Some(Object(cfg)) => Some(cfg),
            _ => None,
        };
        let functions = functions_on.then(|| FunctionRules {
            schemas: schemas(publish, function_cfg.map_or(&[], |cfg| cfg.from_schemas.as_slice())),
            id_format: function_cfg
                .and_then(|cfg| cfg.source_id_format.as_ref())
                .map_or("{function}", IdFormat::as_str)
                .to_owned(),
        });

        Self {
            tables,
            functions,
            configured_tables: config
                .tables
                .iter()
                .flatten()
                .map(|(_, t)| TableKey {
                    schema: t.schema.clone(),
                    table: t.table.clone(),
                    geometry_column: t.geometry_column.clone(),
                })
                .collect(),
            configured_functions: config
                .functions
                .iter()
                .flatten()
                .map(|(_, f)| FunctionKey {
                    schema: f.schema.clone(),
                    function: f.function.clone(),
                })
                .collect(),
        }
    }

    /// The catalog queries that configured and auto-published sources need.
    #[must_use]
    pub fn reads(&self) -> CatalogReads {
        let tables = (self.tables.is_some() || !self.configured_tables.is_empty()).then(|| {
            let configured = self.configured_tables.iter().map(|key| &key.schema);
            let schemas = match &self.tables {
                Some(TableRules { schemas: None, .. }) => None,
                Some(TableRules {
                    schemas: Some(auto), ..
                }) => Some(lowercase(auto.iter().chain(configured))),
                None => Some(lowercase(configured)),
            };
            let only = self.tables.is_none().then(|| {
                self.configured_tables
                    .iter()
                    .map(|key| (key.schema.to_lowercase(), key.table.to_lowercase()))
                    .collect()
            });
            TableReads { schemas, only }
        });
        CatalogReads {
            tables,
            functions: self.functions.is_some() || !self.configured_functions.is_empty(),
        }
    }

    /// The conversions [`plan`](Self::plan) looks up in [`Catalog::transforms`].
    #[must_use]
    pub fn transforms(
        &self,
        tables: &SqlTableInfoMapMapMap,
        grid: &PgTileGrid,
        default_srid: Option<i32>,
    ) -> HashSet<Transform> {
        let Some(rules) = &self.tables else {
            return HashSet::new();
        };
        self.candidate_tables(rules, tables)
            .1
            .into_iter()
            .filter_map(|(_, info)| table_srid(info.srid, grid, default_srid).ok())
            .filter_map(|srid| transform(srid, grid))
            .collect()
    }

    /// The tables and functions to auto-publish from `catalog`.
    ///
    /// # Panics
    /// Panics if `catalog.transforms` lacks a pair that [`transforms`](Self::transforms) returned.
    #[must_use]
    pub fn plan(&self, catalog: &Catalog, grid: &PgTileGrid, default_srid: Option<i32>) -> Plan {
        let mut plan = Plan::default();
        if let Some(rules) = &self.tables {
            self.plan_tables(rules, catalog, grid, default_srid, &mut plan);
        }
        if let Some(rules) = &self.functions {
            self.plan_functions(rules, &catalog.functions, &mut plan);
        }
        plan
    }

    fn plan_tables(
        &self,
        rules: &TableRules,
        catalog: &Catalog,
        grid: &PgTileGrid,
        default_srid: Option<i32>,
        plan: &mut Plan,
    ) {
        let (schemas, tables) = self.candidate_tables(rules, &catalog.tables);
        plan.decisions.push(Decision::ScannedTables {
            schemas,
            id_format: rules.id_format.clone(),
        });
        for (relation, info) in tables {
            let srid = match table_srid(info.srid, grid, default_srid) {
                Ok(srid) => srid,
                Err(reason) => {
                    plan.decisions.push(Decision::Skipped { relation, reason });
                    continue;
                }
            };
            if srid != info.srid {
                plan.decisions.push(Decision::DefaultSrid {
                    relation: relation.clone(),
                    srid,
                });
            }
            if let Some(transform) = transform(srid, grid) {
                let possible = catalog
                    .transforms
                    .get(&transform)
                    .expect("every transform the plan needs was probed");
                if !possible {
                    plan.decisions.push(Decision::Skipped {
                        relation,
                        reason: SkipReason::CannotTransform(transform),
                    });
                    continue;
                }
            }
            let mut info = info.clone();
            info.srid = srid;
            rules.fill_defaults(&relation, &mut info, &mut plan.decisions);
            plan.tables.push((rules.id(&relation), info));
        }
    }

    fn plan_functions(&self, rules: &FunctionRules, functions: &SqlFuncInfoMapMap, plan: &mut Plan) {
        let configured: HashSet<FunctionKey> = self
            .configured_functions
            .iter()
            .filter_map(|key| {
                let (schema, functions) = resolve(functions, &key.schema)?;
                let (function, _) = resolve(functions, &key.function)?;
                Some(FunctionKey {
                    schema: schema.clone(),
                    function: function.clone(),
                })
            })
            .collect();
        let scanned = scanned_schemas(functions, rules.schemas.as_ref());
        plan.decisions.push(Decision::ScannedFunctions {
            schemas: scanned.iter().map(|(schema, _)| (*schema).clone()).collect(),
            id_format: rules.id_format.clone(),
        });
        for (schema, functions) in scanned {
            for (function, (sql, info)) in functions {
                let key = FunctionKey {
                    schema: schema.clone(),
                    function: function.clone(),
                };
                if configured.contains(&key) {
                    continue;
                }
                let id = rules
                    .id_format
                    .replace("{schema}", schema)
                    .replace("{function}", function_name(function));
                plan.functions.push((id, info.clone(), sql.clone()));
            }
        }
    }

    /// The scanned schemas, and the tables in them that no configured source names.
    fn candidate_tables<'a>(
        &self,
        rules: &TableRules,
        tables: &'a SqlTableInfoMapMapMap,
    ) -> (Vec<String>, Vec<(TableKey, &'a TableInfo)>) {
        let configured: HashSet<TableKey> = self
            .configured_tables
            .iter()
            .filter_map(|key| {
                let (schema, in_schema) = resolve(tables, &key.schema)?;
                let (table, columns) = resolve(in_schema, &key.table)?;
                let (geometry_column, _) = resolve(columns, &key.geometry_column)?;
                Some(TableKey {
                    schema: schema.clone(),
                    table: table.clone(),
                    geometry_column: geometry_column.clone(),
                })
            })
            .collect();
        let scanned = scanned_schemas(tables, rules.schemas.as_ref());
        let names = scanned.iter().map(|(schema, _)| (*schema).clone()).collect();
        let mut candidates = Vec::new();
        for (schema, in_schema) in scanned {
            for (table, columns) in in_schema {
                for (geometry_column, info) in columns {
                    let key = TableKey {
                        schema: schema.clone(),
                        table: table.clone(),
                        geometry_column: geometry_column.clone(),
                    };
                    if !configured.contains(&key) {
                        candidates.push((key, info));
                    }
                }
            }
        }
        (names, candidates)
    }
}

impl TableRules {
    /// The id a table asks for.
    fn id(&self, relation: &TableKey) -> String {
        self.id_format
            .replace("{schema}", &relation.schema)
            .replace("{table}", &relation.table)
            .replace("{column}", &relation.geometry_column)
    }

    /// Fills in the clip, buffer and extent defaults and the first usable feature id column.
    fn fill_defaults(&self, relation: &TableKey, info: &mut TableInfo, decisions: &mut Vec<Decision>) {
        info.clip_geom = info.clip_geom.or(self.clip_geom);
        info.buffer = info.buffer.or(self.buffer);
        info.extent = info.extent.or(self.extent);

        if self.id_columns.is_empty() {
            return;
        }
        let Some(props) = info.properties.as_mut() else {
            return;
        };
        let mut report = |outcome| {
            decisions.push(Decision::IdColumn {
                relation: relation.clone(),
                outcome,
            });
        };
        for requested in &self.id_columns {
            let column = if props.contains_key(requested) {
                requested.clone()
            } else {
                match find_kv_ignore_case(props, requested) {
                    Ok(Some(found)) => {
                        report(IdColumnOutcome::CaseInsensitive {
                            requested: requested.clone(),
                            found: found.clone(),
                        });
                        found.clone()
                    }
                    Ok(None) => continue,
                    Err(candidates) => {
                        report(IdColumnOutcome::Ambiguous {
                            requested: requested.clone(),
                            candidates,
                        });
                        continue;
                    }
                }
            };
            // https://github.com/postgis/postgis/blob/559c95d85564fb74fa9e3b7eafb74851810610da/postgis/mvt.c#L387C4-L387C66
            let typ = props.get(&column).map_or("", String::as_str);
            if !matches!(typ, "int2" | "int4" | "int8") {
                report(IdColumnOutcome::NotInteger {
                    typ: typ.to_owned(),
                    column,
                });
                continue;
            }
            props.remove(&column);
            info.id_column = Some(column);
            return;
        }
        report(IdColumnOutcome::NoneFound {
            searched: self.id_columns.clone(),
        });
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

/// The outer and inner `from_schemas` together, or `None` when neither names any.
fn schemas(publish: Option<&PostgresCfgPublish>, inner: &[String]) -> Option<HashSet<String>> {
    let schemas: HashSet<String> = publish?.from_schemas.iter().chain(inner).cloned().collect();
    (!schemas.is_empty()).then_some(schemas)
}

/// The names lowercased.
fn lowercase<'a>(names: impl Iterator<Item = &'a String>) -> Vec<String> {
    names.map(|name| name.to_lowercase()).collect()
}

/// The catalog schemas that `wanted` names, or all of them when it is `None`.
fn scanned_schemas<'a, T>(
    catalog: &'a BTreeMap<String, T>,
    wanted: Option<&HashSet<String>>,
) -> Vec<(&'a String, &'a T)> {
    let Some(wanted) = wanted else {
        return catalog.iter().collect();
    };
    let found: HashSet<&String> = wanted
        .iter()
        .filter_map(|schema| resolve(catalog, schema))
        .map(|(schema, _)| schema)
        .collect();
    catalog.iter().filter(|(schema, _)| found.contains(schema)).collect()
}

/// The entry for `key`, else the only entry whose key matches it ignoring case.
fn resolve<'a, T>(map: &'a BTreeMap<String, T>, key: &str) -> Option<(&'a String, &'a T)> {
    if let Some(found) = map.get_key_value(key) {
        return Some(found);
    }
    let key = find_kv_ignore_case(map, key).ok()??;
    map.get_key_value(key)
}

/// The SRID an auto-published table is read in.
fn table_srid(found: i32, grid: &PgTileGrid, default_srid: Option<i32>) -> Result<i32, SkipReason> {
    if grid.grid().is_simple() {
        return if found == 0 {
            Ok(0)
        } else {
            Err(SkipReason::SridOnSimpleGrid(found))
        };
    }
    match (found, default_srid) {
        (0, None) => Err(SkipReason::NoSrid),
        (0, Some(srid)) | (srid, _) => Ok(srid),
    }
}

/// The conversion a table read in `srid` needs to be served in `grid`, if any.
fn transform(srid: i32, grid: &PgTileGrid) -> Option<Transform> {
    (srid != grid.srid()).then_some(Transform {
        table: srid,
        grid: grid.srid(),
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use martin_tile_utils::TileGrid;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::config::file::postgres::Srid;

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Fixture {
        config: PostgresConfig,
        grid: Option<FixtureGrid>,
        #[serde(default)]
        catalog: FixtureCatalog,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FixtureGrid {
        crs: String,
        srid: i32,
    }

    #[derive(Default, Deserialize)]
    #[serde(default, deny_unknown_fields)]
    struct FixtureCatalog {
        tables: Vec<TableInfo>,
        functions: Vec<FixtureFunction>,
        transforms: Vec<FixtureTransform>,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FixtureFunction {
        schema: String,
        name: String,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FixtureTransform {
        table: i32,
        grid: i32,
        ok: bool,
    }

    #[derive(Serialize)]
    struct Snapshot {
        reads: CatalogReads,
        transforms: HashSet<Transform>,
        tables: Vec<Published<TableInfo>>,
        functions: Vec<Published<FunctionInfo>>,
        decisions: Vec<Decision>,
    }

    #[derive(Serialize)]
    struct Published<T> {
        id: String,
        #[serde(flatten)]
        info: T,
    }

    #[test]
    fn plan_matches_each_fixture() {
        insta::glob!(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../tests/fixtures/pg_auto_publish"),
            "*.yaml",
            |path| {
                let text = fs::read_to_string(path).expect("read fixture");
                let fixture: Fixture = serde_saphyr::from_str(&text).expect("parse fixture");
                let grid = fixture.grid.map_or_else(PgTileGrid::web_mercator, |grid| {
                    let tile_grid = TileGrid::new("FixtureGrid", grid.crs, [0.0, 1000.0], 1000.0).expect("valid grid");
                    PgTileGrid::new(tile_grid, grid.srid)
                });
                let default_srid = fixture.config.default_srid.map(Srid::get);

                let mut catalog = Catalog::default();
                for info in fixture.catalog.tables {
                    catalog
                        .tables
                        .entry(info.schema.clone())
                        .or_default()
                        .entry(info.table.clone())
                        .or_default()
                        .insert(info.geometry_column.clone(), info);
                }
                for function in fixture.catalog.functions {
                    let sql = PostgresSqlInfo::new(String::new(), false, false, function.name.clone(), false);
                    let info = FunctionInfo::new(function.schema.clone(), function.name.clone(), None);
                    catalog
                        .functions
                        .entry(function.schema)
                        .or_default()
                        .insert(function.name, (sql, info));
                }
                catalog.transforms = fixture
                    .catalog
                    .transforms
                    .into_iter()
                    .map(|t| {
                        (
                            Transform {
                                table: t.table,
                                grid: t.grid,
                            },
                            t.ok,
                        )
                    })
                    .collect();

                let auto = AutoPublish::new(&fixture.config);
                let transforms = auto.transforms(&catalog.tables, &grid, default_srid);
                let plan = auto.plan(&catalog, &grid, default_srid);
                let snapshot = Snapshot {
                    reads: auto.reads(),
                    transforms,
                    tables: plan
                        .tables
                        .into_iter()
                        .map(|(id, info)| Published { id, info })
                        .collect(),
                    functions: plan
                        .functions
                        .into_iter()
                        .map(|(id, info, _)| Published { id, info })
                        .collect(),
                    decisions: plan.decisions,
                };
                insta::assert_yaml_snapshot!(snapshot, {
                    ".reads.tables.schemas" => insta::sorted_redaction(),
                    ".reads.tables.only" => insta::sorted_redaction(),
                    ".transforms" => insta::sorted_redaction(),
                });
            }
        );
    }
}
