use std::cmp::Ordering;
use std::collections::HashSet;

use futures::future::join_all;
use itertools::Itertools;
use log::{debug, error, info, warn};

use crate::args::BoundsCalcType;
use crate::pg::config::{PgConfig, PgInfo};
use crate::pg::config_function::{FuncInfoSources, FunctionInfo};
use crate::pg::config_table::{TableInfo, TableInfoSources};
use crate::pg::function_source::query_available_function;
use crate::pg::pg_source::{PgSource, PgSqlInfo};
use crate::pg::pool::PgPool;
use crate::pg::table_source::{
    calc_srid, merge_table_info, query_available_tables, table_to_query,
};
use crate::pg::utils::{find_info, find_kv_ignore_case, normalize_key, InfoMap};
use crate::pg::PgError::InvalidTableExtent;
use crate::pg::{PgCfgPublish, PgCfgPublishFuncs, Result};
use crate::source::TileInfoSources;
use crate::utils::IdResolver;
use crate::utils::OptOneMany::NoVals;
use crate::OptBoolObj::{Bool, NoValue, Object};

pub type SqlFuncInfoMapMap = InfoMap<InfoMap<(PgSqlInfo, FunctionInfo)>>;
pub type SqlTableInfoMapMapMap = InfoMap<InfoMap<InfoMap<TableInfo>>>;

#[derive(Debug, PartialEq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub struct PgBuilderFuncs {
    #[cfg_attr(test, serde(skip_serializing_if = "Option::is_none"))]
    schemas: Option<HashSet<String>>,
    source_id_format: String,
}

#[derive(Debug, Default, PartialEq)]
#[cfg_attr(test, derive(serde::Serialize))]
pub struct PgBuilderTables {
    #[cfg_attr(
        test,
        serde(
            skip_serializing_if = "Option::is_none",
            serialize_with = "crate::utils::sorted_opt_set"
        )
    )]
    schemas: Option<HashSet<String>>,
    source_id_format: String,
    #[cfg_attr(test, serde(skip_serializing_if = "Option::is_none"))]
    id_columns: Option<Vec<String>>,
    #[cfg_attr(test, serde(skip_serializing_if = "Option::is_none"))]
    clip_geom: Option<bool>,
    #[cfg_attr(test, serde(skip_serializing_if = "Option::is_none"))]
    buffer: Option<u32>,
    #[cfg_attr(test, serde(skip_serializing_if = "Option::is_none"))]
    extent: Option<u32>,
}

#[derive(Debug)]
pub struct PgBuilder {
    pool: PgPool,
    default_srid: Option<i32>,
    bounds: BoundsCalcType,
    max_feature_count: Option<usize>,
    auto_functions: Option<PgBuilderFuncs>,
    auto_tables: Option<PgBuilderTables>,
    id_resolver: IdResolver,
    tables: TableInfoSources,
    functions: FuncInfoSources,
}

/// Combine `from_schema` field from the `config.auto_publish` and `config.auto_publish.tables/functions`
macro_rules! get_auto_schemas {
    ($config:expr, $typ:ident) => {
        if let Object(v) = &$config.auto_publish {
            match (&v.from_schemas, &v.$typ) {
                (NoVals, NoValue | Bool(_)) => None,
                (v, NoValue | Bool(_)) => v.opt_iter().map(|v| v.cloned().collect()),
                (NoVals, Object(v)) => v.from_schemas.opt_iter().map(|v| v.cloned().collect()),
                (v, Object(v2)) => {
                    let mut vals: HashSet<_> = v.iter().cloned().collect();
                    vals.extend(v2.from_schemas.iter().cloned());
                    Some(vals)
                }
            }
        } else {
            None
        }
    };
}

impl PgBuilder {
    pub async fn new(config: &PgConfig, id_resolver: IdResolver) -> Result<Self> {
        let pool = PgPool::new(config).await?;

        let (auto_tables, auto_functions) = calc_auto(config);

        Ok(Self {
            pool,
            default_srid: config.default_srid,
            bounds: config.bounds.unwrap_or_default(),
            max_feature_count: config.max_feature_count,
            id_resolver,
            tables: config.tables.clone().unwrap_or_default(),
            functions: config.functions.clone().unwrap_or_default(),
            auto_functions,
            auto_tables,
        })
    }

    pub fn bounds(&self) -> BoundsCalcType {
        self.bounds
    }

    pub fn get_id(&self) -> &str {
        self.pool.get_id()
    }

    // FIXME: this function has gotten too long due to the new formatting rules, need to be refactored
    #[allow(clippy::too_many_lines)]
    pub async fn instantiate_tables(&self) -> Result<(TileInfoSources, TableInfoSources)> {
        let mut db_tables_info = query_available_tables(&self.pool).await?;

        // Match configured sources with the discovered ones and add them to the pending list.
        let mut used = HashSet::<(&str, &str, &str)>::new();
        let mut pending = Vec::new();
        for (id, cfg_inf) in &self.tables {
            // TODO: move this validation to serde somehow?
            if let Some(extent) = cfg_inf.extent {
                if extent == 0 {
                    return Err(InvalidTableExtent(id.to_string(), cfg_inf.format_id()));
                }
            }

            let Some(db_tables) = find_info(&db_tables_info, &cfg_inf.schema, "schema", id) else {
                continue;
            };
            let Some(db_geo_columns) = find_info(db_tables, &cfg_inf.table, "table", id) else {
                continue;
            };
            let Some(db_inf) = find_info(
                db_geo_columns,
                &cfg_inf.geometry_column,
                "geometry column",
                id,
            ) else {
                continue;
            };

            let dup = !used.insert((&cfg_inf.schema, &cfg_inf.table, &cfg_inf.geometry_column));
            let dup = if dup { "duplicate " } else { "" };

            let id2 = self.resolve_id(id, cfg_inf);
            let Some(merged_inf) = merge_table_info(self.default_srid, &id2, cfg_inf, db_inf)
            else {
                continue;
            };
            warn_on_rename(id, &id2, "Table");
            info!("Configured {dup}source {id2} from {}", summary(&merged_inf));
            pending.push(table_to_query(
                id2,
                merged_inf,
                self.pool.clone(),
                self.bounds,
                self.max_feature_count,
            ));
        }

        // Sort the discovered sources by schema, table and geometry column to ensure a consistent behavior
        if let Some(auto_tables) = &self.auto_tables {
            let schemas = auto_tables
                .schemas
                .as_ref()
                .cloned()
                .unwrap_or_else(|| db_tables_info.keys().cloned().collect());
            info!(
                "Auto-publishing tables in schemas [{}] as '{}' sources",
                schemas.iter().sorted().join(", "),
                auto_tables.source_id_format,
            );

            for schema in schemas.iter().sorted() {
                let Some(schema) = normalize_key(&db_tables_info, schema, "schema", "") else {
                    continue;
                };
                let db_tables = db_tables_info.remove(&schema).unwrap();
                for (table, geoms) in db_tables.into_iter().sorted_by(by_key) {
                    for (geom_column, mut db_inf) in geoms.into_iter().sorted_by(by_key) {
                        if used.contains(&(schema.as_str(), table.as_str(), geom_column.as_str())) {
                            continue;
                        }
                        let source_id = auto_tables
                            .source_id_format
                            .replace("{schema}", &schema)
                            .replace("{table}", &table)
                            .replace("{column}", &geom_column);
                        let id2 = self.resolve_id(&source_id, &db_inf);
                        let Some(srid) =
                            calc_srid(&db_inf.format_id(), &id2, db_inf.srid, 0, self.default_srid)
                        else {
                            continue;
                        };
                        db_inf.srid = srid;
                        update_auto_fields(&id2, &mut db_inf, auto_tables);
                        info!("Discovered source {id2} from {}", summary(&db_inf));
                        pending.push(table_to_query(
                            id2,
                            db_inf,
                            self.pool.clone(),
                            self.bounds,
                            self.max_feature_count,
                        ));
                    }
                }
            }
        }

        let mut res = TileInfoSources::default();
        let mut info_map = TableInfoSources::new();
        let pending = join_all(pending).await;
        for src in pending {
            match src {
                Err(v) => {
                    error!("Failed to create a source: {v}");
                    continue;
                }
                Ok((id, pg_sql, src_inf)) => {
                    debug!("{id} query: {}", pg_sql.query);
                    self.add_func_src(&mut res, id.clone(), &src_inf, pg_sql.clone());
                    info_map.insert(id, src_inf);
                }
            }
        }

        Ok((res, info_map))
    }

    pub async fn instantiate_functions(&self) -> Result<(TileInfoSources, FuncInfoSources)> {
        let mut db_funcs_info = query_available_function(&self.pool).await?;
        let mut res = TileInfoSources::default();
        let mut info_map = FuncInfoSources::new();
        let mut used = HashSet::<(&str, &str)>::new();

        for (id, cfg_inf) in &self.functions {
            let Some(db_funcs) = find_info(&db_funcs_info, &cfg_inf.schema, "schema", id) else {
                continue;
            };
            if db_funcs.is_empty() {
                warn!("No functions found in schema {}. Only functions like (z,x,y) -> bytea and similar are considered. See README.md", cfg_inf.schema);
                continue;
            }
            let Some((pg_sql, _)) = find_info(db_funcs, &cfg_inf.function, "function", id) else {
                continue;
            };

            let dup = !used.insert((&cfg_inf.schema, &cfg_inf.function));
            let dup = if dup { "duplicate " } else { "" };

            let id2 = self.resolve_id(id, cfg_inf);
            self.add_func_src(&mut res, id2.clone(), cfg_inf, pg_sql.clone());
            warn_on_rename(id, &id2, "Function");
            let signature = &pg_sql.signature;
            info!("Configured {dup}source {id2} from the function {signature}");
            debug!("{id2} query: {}", pg_sql.query);
            info_map.insert(id2, cfg_inf.clone());
        }

        // Sort the discovered sources by schema and function name to ensure a consistent behavior
        if let Some(auto_funcs) = &self.auto_functions {
            let schemas = auto_funcs
                .schemas
                .as_ref()
                .cloned()
                .unwrap_or_else(|| db_funcs_info.keys().cloned().collect());
            info!(
                "Auto-publishing functions in schemas [{}] as '{}' sources",
                schemas.iter().sorted().join(", "),
                auto_funcs.source_id_format,
            );

            for schema in schemas.iter().sorted() {
                let Some(schema) = normalize_key(&db_funcs_info, schema, "schema", "") else {
                    continue;
                };
                let db_funcs = db_funcs_info.remove(&schema).unwrap();
                for (func, (pg_sql, db_inf)) in db_funcs.into_iter().sorted_by(by_key) {
                    if used.contains(&(schema.as_str(), func.as_str())) {
                        continue;
                    }
                    let source_id = auto_funcs
                        .source_id_format
                        .replace("{schema}", &schema)
                        .replace("{function}", &func);
                    let id2 = self.resolve_id(&source_id, &db_inf);
                    self.add_func_src(&mut res, id2.clone(), &db_inf, pg_sql.clone());
                    info!("Discovered source {id2} from function {}", pg_sql.signature);
                    debug!("{id2} query: {}", pg_sql.query);
                    info_map.insert(id2, db_inf);
                }
            }
        }
        Ok((res, info_map))
    }

    fn resolve_id<T: PgInfo>(&self, id: &str, src_inf: &T) -> String {
        let signature = format!("{}.{}", self.pool.get_id(), src_inf.format_id());
        self.id_resolver.resolve(id, signature)
    }

    fn add_func_src(
        &self,
        sources: &mut TileInfoSources,
        id: String,
        info: &impl PgInfo,
        sql: PgSqlInfo,
    ) {
        let tilejson = info.to_tilejson(id.clone());
        let source = PgSource::new(id, sql, tilejson, self.pool.clone());
        sources.push(Box::new(source));
    }
}

fn update_auto_fields(id: &str, inf: &mut TableInfo, auto_tables: &PgBuilderTables) {
    if inf.clip_geom.is_none() {
        inf.clip_geom = auto_tables.clip_geom;
    }
    if inf.buffer.is_none() {
        inf.buffer = auto_tables.buffer;
    }
    if inf.extent.is_none() {
        inf.extent = auto_tables.extent;
    }

    // Try to find any ID column in a list of table columns (properties) that match one of the given `id_column` values.
    // If found, modify `id_column` value on the table info.
    let Some(props) = inf.properties.as_mut() else {
        return;
    };
    let Some(try_columns) = &auto_tables.id_columns else {
        return;
    };

    for key in try_columns {
        let (column, typ) = if let Some(typ) = props.get(key) {
            (key, typ)
        } else {
            match find_kv_ignore_case(props, key) {
                Ok(Some(result)) => {
                    info!("For source {id}, id_column '{key}' was not found, but found '{result}' instead.");
                    (result, props.get(result).unwrap())
                }
                Ok(None) => continue,
                Err(multiple) => {
                    error!("Unable to configure source {id} because id_column '{key}' has no exact match or more than one potential matches: {}", multiple.join(", "));
                    continue;
                }
            }
        };
        // ID column can be any integer type as defined in
        // https://github.com/postgis/postgis/blob/559c95d85564fb74fa9e3b7eafb74851810610da/postgis/mvt.c#L387C4-L387C66
        if typ != "int4" && typ != "int8" && typ != "int2" {
            warn!("Unable to use column `{key}` in table {}.{} as a tile feature ID because it has a non-integer type `{typ}`.", inf.schema, inf.table);
            continue;
        }

        inf.id_column = Some(column.to_string());
        let mut final_props = props.clone();
        final_props.remove(column);
        inf.properties = Some(final_props);
        return;
    }

    info!(
        "No ID column found for table {}.{} - searched for an integer column named {}.",
        inf.schema,
        inf.table,
        try_columns.join(", ")
    );
}

fn calc_auto(config: &PgConfig) -> (Option<PgBuilderTables>, Option<PgBuilderFuncs>) {
    let auto_tables = if use_auto_publish(config, false) {
        let schemas = get_auto_schemas!(config, tables);
        let bld = if let Object(PgCfgPublish {
            tables: Object(v), ..
        }) = &config.auto_publish
        {
            PgBuilderTables {
                schemas,
                source_id_format: v
                    .source_id_format
                    .as_deref()
                    .unwrap_or("{table}")
                    .to_string(),
                id_columns: v.id_columns.opt_iter().map(|v| v.cloned().collect()),
                clip_geom: v.clip_geom,
                buffer: v.buffer,
                extent: v.extent,
            }
        } else {
            PgBuilderTables {
                schemas,
                source_id_format: "{table}".to_string(),
                ..Default::default()
            }
        };
        Some(bld)
    } else {
        None
    };

    let auto_functions = if use_auto_publish(config, true) {
        Some(PgBuilderFuncs {
            schemas: get_auto_schemas!(config, functions),
            source_id_format: if let Object(PgCfgPublish {
                functions:
                    Object(PgCfgPublishFuncs {
                        source_id_format: Some(v),
                        ..
                    }),
                ..
            }) = &config.auto_publish
            {
                v.clone()
            } else {
                "{function}".to_string()
            },
        })
    } else {
        None
    };

    (auto_tables, auto_functions)
}

fn use_auto_publish(config: &PgConfig, for_functions: bool) -> bool {
    match &config.auto_publish {
        NoValue => config.tables.is_none() && config.functions.is_none(),
        Object(funcs) => {
            if for_functions {
                // If auto_publish.functions is set, and currently asking for .tables which is missing,
                // .tables becomes the inverse of functions (i.e. an obj or true in tables means false in functions)
                match &funcs.functions {
                    NoValue => matches!(funcs.tables, NoValue | Bool(false)),
                    Object(_) => true,
                    Bool(v) => *v,
                }
            } else {
                match &funcs.tables {
                    NoValue => matches!(funcs.functions, NoValue | Bool(false)),
                    Object(_) => true,
                    Bool(v) => *v,
                }
            }
        }
        Bool(v) => *v,
    }
}

fn warn_on_rename(old_id: &String, new_id: &String, typ: &str) {
    if old_id != new_id {
        warn!("{typ} source {old_id} was renamed to {new_id} due to ID conflict");
    }
}

fn summary(info: &TableInfo) -> String {
    let relkind = match info.is_view {
        Some(true) => "view",
        _ => "table",
    };
    // TODO: add column_id to the summary if it is set
    format!(
        "{relkind} {}.{} with {} column ({}, SRID={})",
        info.schema,
        info.table,
        info.geometry_column,
        info.geometry_type
            .as_deref()
            .unwrap_or("UNKNOWN GEOMETRY TYPE"),
        info.srid,
    )
}

/// A comparator for sorting tuples by first element
fn by_key<T>(a: &(String, T), b: &(String, T)) -> Ordering {
    a.0.cmp(&b.0)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;
    use insta::assert_yaml_snapshot;

    use super::*;

    #[derive(serde::Serialize)]
    struct AutoCfg {
        auto_table: Option<PgBuilderTables>,
        auto_funcs: Option<PgBuilderFuncs>,
    }
    fn auto(content: &str) -> AutoCfg {
        let cfg: PgConfig = serde_yaml::from_str(content).unwrap();
        let (auto_table, auto_funcs) = calc_auto(&cfg);
        AutoCfg {
            auto_table,
            auto_funcs,
        }
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn test_auto_publish_no_auto() {
        let cfg = auto("{}");
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table:
              source_id_format: "{table}"
            auto_funcs:
              source_id_format: "{function}"
            "###);

        let cfg = auto("tables: {}");
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table: ~
            auto_funcs: ~
            "###);

        let cfg = auto("functions: {}");
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table: ~
            auto_funcs: ~
            "###);

        let cfg = auto("auto_publish: true");
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table:
              source_id_format: "{table}"
            auto_funcs:
              source_id_format: "{function}"
            "###);

        let cfg = auto("auto_publish: false");
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table: ~
            auto_funcs: ~
            "###);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                tables: true"});
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table:
              schemas:
                - public
              source_id_format: "{table}"
            auto_funcs: ~
            "###);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                functions: true"});
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table: ~
            auto_funcs:
              schemas:
                - public
              source_id_format: "{function}"
            "###);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                tables: false"});
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table: ~
            auto_funcs:
              schemas:
                - public
              source_id_format: "{function}"
            "###);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                functions: false"});
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table:
              schemas:
                - public
              source_id_format: "{table}"
            auto_funcs: ~
            "###);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                tables:
                    from_schemas: osm
                    id_format: 'foo_{schema}.{table}_bar'"});
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table:
              schemas:
                - osm
                - public
              source_id_format: "foo_{schema}.{table}_bar"
            auto_funcs: ~
            "###);

        let cfg = auto(indoc! {"
            auto_publish:
                from_schemas: public
                tables:
                    from_schemas: osm
                    source_id_format: '{schema}.{table}'"});
        assert_yaml_snapshot!(cfg, @r###"
        ---
        auto_table:
          schemas:
            - osm
            - public
          source_id_format: "{schema}.{table}"
        auto_funcs: ~
        "###);

        let cfg = auto(indoc! {"
            auto_publish:
                tables:
                    from_schemas:
                      - osm
                      - public"});
        assert_yaml_snapshot!(cfg, @r###"
            ---
            auto_table:
              schemas:
                - osm
                - public
              source_id_format: "{table}"
            auto_funcs: ~
            "###);
    }
}
