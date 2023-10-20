use std::cmp::Ordering;
use std::collections::HashSet;

use crate::OptBoolObj::{Bool, NoValue, Object};
use futures::future::join_all;
use itertools::Itertools;
use log::{debug, error, info, warn};

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
use crate::pg::Result;
use crate::source::TileInfoSources;
use crate::utils::{IdResolver, OptOneMany};

pub type SqlFuncInfoMapMap = InfoMap<InfoMap<(PgSqlInfo, FunctionInfo)>>;
pub type SqlTableInfoMapMapMap = InfoMap<InfoMap<InfoMap<TableInfo>>>;

#[derive(Debug, PartialEq)]
pub struct PgBuilderAuto {
    source_id_format: String,
    schemas: Option<HashSet<String>>,
    id_columns: Option<Vec<String>>,
    clip_geom: Option<bool>,
    buffer: Option<u32>,
    extent: Option<u32>,
}

#[derive(Debug)]
pub struct PgBuilder {
    pool: PgPool,
    default_srid: Option<i32>,
    disable_bounds: bool,
    max_feature_count: Option<usize>,
    auto_functions: Option<PgBuilderAuto>,
    auto_tables: Option<PgBuilderAuto>,
    id_resolver: IdResolver,
    tables: TableInfoSources,
    functions: FuncInfoSources,
}

impl PgBuilder {
    pub async fn new(config: &PgConfig, id_resolver: IdResolver) -> Result<Self> {
        let pool = PgPool::new(config).await?;

        Ok(Self {
            pool,
            default_srid: config.default_srid,
            disable_bounds: config.disable_bounds.unwrap_or_default(),
            max_feature_count: config.max_feature_count,
            id_resolver,
            tables: config.tables.clone().unwrap_or_default(),
            functions: config.functions.clone().unwrap_or_default(),
            auto_functions: new_auto_publish(config, true),
            auto_tables: new_auto_publish(config, false),
        })
    }

    pub fn disable_bounds(&self) -> bool {
        self.disable_bounds
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
                self.disable_bounds,
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
                            self.disable_bounds,
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

fn update_auto_fields(id: &str, inf: &mut TableInfo, auto_tables: &PgBuilderAuto) {
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

fn new_auto_publish(config: &PgConfig, is_function: bool) -> Option<PgBuilderAuto> {
    let default_id_fmt = |is_func| (if is_func { "{function}" } else { "{table}" }).to_string();
    let default = |schemas| {
        Some(PgBuilderAuto {
            source_id_format: default_id_fmt(is_function),
            schemas,
            id_columns: None,
            clip_geom: None,
            buffer: None,
            extent: None,
        })
    };

    match &config.auto_publish {
        NoValue => {
            if config.tables.is_some() || config.functions.is_some() {
                None
            } else {
                default(None)
            }
        }
        Object(a) => match if is_function { &a.functions } else { &a.tables } {
            // If auto_publish.functions is set, and currently asking for .tables which is missing,
            // .tables becomes the inverse of functions (i.e. an obj or true in tables means false in functions)
            NoValue => match if is_function { &a.tables } else { &a.functions } {
                NoValue | Bool(false) => {
                    default(merge_opt_hs(&a.from_schemas, &OptOneMany::NoValue))
                }
                Object(_) | Bool(true) => None,
            },
            Object(item) => Some(PgBuilderAuto {
                source_id_format: item
                    .source_id_format
                    .as_ref()
                    .cloned()
                    .unwrap_or_else(|| default_id_fmt(is_function)),
                schemas: merge_opt_hs(&a.from_schemas, &item.from_schemas),
                id_columns: {
                    if item.id_columns.is_none() {
                        None
                    } else if is_function {
                        error!("Configuration parameter auto_publish.functions.id_columns is not supported");
                        None
                    } else {
                        Some(item.id_columns.iter().cloned().collect())
                    }
                },
                clip_geom: {
                    if is_function {
                        error!("Configuration parameter auto_publish.functions.clip_geom is not supported");
                        None
                    } else {
                        item.clip_geom
                    }
                },
                buffer: {
                    if is_function {
                        error!("Configuration parameter auto_publish.functions.buffer is not supported");
                        None
                    } else {
                        item.buffer
                    }
                },
                extent: {
                    if is_function {
                        error!("Configuration parameter auto_publish.functions.extent is not supported");
                        None
                    } else {
                        item.extent
                    }
                },
            }),
            Bool(true) => default(merge_opt_hs(&a.from_schemas, &OptOneMany::NoValue)),
            Bool(false) => None,
        },
        Bool(true) => default(None),
        Bool(false) => None,
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

/// Merge two optional list of strings into a hashset
fn merge_opt_hs(a: &OptOneMany<String>, b: &OptOneMany<String>) -> Option<HashSet<String>> {
    match (a.is_none(), b.is_none()) {
        (true, true) => None,
        (true, false) => Some(b.iter().cloned().collect()),
        (false, true) => Some(a.iter().cloned().collect()),
        (false, false) => {
            let mut res: HashSet<_> = a.iter().cloned().collect();
            res.extend(b.iter().cloned());
            Some(res)
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[allow(clippy::unnecessary_wraps)]
    fn builder(source_id_format: &str, schemas: Option<&[&str]>) -> Option<PgBuilderAuto> {
        Some(PgBuilderAuto {
            source_id_format: source_id_format.to_string(),
            schemas: schemas.map(|s| s.iter().map(|s| (*s).to_string()).collect()),
            id_columns: None,
            clip_geom: None,
            buffer: None,
            extent: None,
        })
    }

    fn parse_yaml(content: &str) -> PgConfig {
        serde_yaml::from_str(content).unwrap()
    }

    #[test]
    fn test_auto_publish_no_auto() {
        let config = parse_yaml("{}");
        let res = new_auto_publish(&config, false);
        assert_eq!(res, builder("{table}", None));
        let res = new_auto_publish(&config, true);
        assert_eq!(res, builder("{function}", None));

        let config = parse_yaml("tables: {}");
        assert_eq!(new_auto_publish(&config, false), None);
        assert_eq!(new_auto_publish(&config, true), None);

        let config = parse_yaml("functions: {}");
        assert_eq!(new_auto_publish(&config, false), None);
        assert_eq!(new_auto_publish(&config, true), None);
    }

    #[test]
    fn test_auto_publish_bool() {
        let config = parse_yaml("auto_publish: true");
        let res = new_auto_publish(&config, false);
        assert_eq!(res, builder("{table}", None));
        let res = new_auto_publish(&config, true);
        assert_eq!(res, builder("{function}", None));

        let config = parse_yaml("auto_publish: false");
        assert_eq!(new_auto_publish(&config, false), None);
        assert_eq!(new_auto_publish(&config, true), None);
    }

    #[test]
    fn test_auto_publish_obj_bool() {
        let config = parse_yaml(indoc! {"
            auto_publish:
                from_schemas: public
                tables: true"});
        let res = new_auto_publish(&config, false);
        assert_eq!(res, builder("{table}", Some(&["public"])));
        assert_eq!(new_auto_publish(&config, true), None);

        let config = parse_yaml(indoc! {"
            auto_publish:
                from_schemas: public
                functions: true"});
        assert_eq!(new_auto_publish(&config, false), None);
        let res = new_auto_publish(&config, true);
        assert_eq!(res, builder("{function}", Some(&["public"])));

        let config = parse_yaml(indoc! {"
            auto_publish:
                from_schemas: public
                tables: false"});
        assert_eq!(new_auto_publish(&config, false), None);
        let res = new_auto_publish(&config, true);
        assert_eq!(res, builder("{function}", Some(&["public"])));

        let config = parse_yaml(indoc! {"
            auto_publish:
                from_schemas: public
                functions: false"});
        let res = new_auto_publish(&config, false);
        assert_eq!(res, builder("{table}", Some(&["public"])));
        assert_eq!(new_auto_publish(&config, true), None);
    }

    #[test]
    fn test_auto_publish_obj_obj() {
        let config = parse_yaml(indoc! {"
            auto_publish:
                from_schemas: public
                tables:
                    from_schemas: osm
                    id_format: 'foo_{schema}.{table}_bar'"});
        let res = new_auto_publish(&config, false);
        assert_eq!(
            res,
            builder("foo_{schema}.{table}_bar", Some(&["public", "osm"]))
        );
        assert_eq!(new_auto_publish(&config, true), None);

        let config = parse_yaml(indoc! {"
            auto_publish:
                from_schemas: public
                tables:
                    from_schemas: osm
                    source_id_format: '{schema}.{table}'"});
        let res = new_auto_publish(&config, false);
        assert_eq!(res, builder("{schema}.{table}", Some(&["public", "osm"])));
        assert_eq!(new_auto_publish(&config, true), None);

        let config = parse_yaml(indoc! {"
            auto_publish:
                tables:
                    from_schemas:
                      - osm
                      - public"});
        let res = new_auto_publish(&config, false);
        assert_eq!(res, builder("{table}", Some(&["public", "osm"])));
        assert_eq!(new_auto_publish(&config, true), None);
    }
}
