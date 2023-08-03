use std::cmp::Ordering;
use std::collections::HashSet;

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
use crate::pg::utils::{find_info, normalize_key, InfoMap};
use crate::pg::PgError::InvalidTableExtent;
use crate::pg::Result;
use crate::source::Sources;
use crate::utils::{BoolOrObject, IdResolver, OneOrMany};

pub type SqlFuncInfoMapMap = InfoMap<InfoMap<(PgSqlInfo, FunctionInfo)>>;
pub type SqlTableInfoMapMapMap = InfoMap<InfoMap<InfoMap<TableInfo>>>;

#[derive(Debug, PartialEq)]
pub struct PgBuilderPublish {
    source_id_format: String,
    schemas: Option<HashSet<String>>,
}

impl PgBuilderPublish {
    pub fn new(
        is_function: bool,
        source_id_format: Option<&String>,
        schemas: Option<HashSet<String>>,
    ) -> Self {
        let source_id_format = source_id_format
            .cloned()
            .unwrap_or_else(|| (if is_function { "{function}" } else { "{table}" }).to_string());
        Self {
            source_id_format,
            schemas,
        }
    }
}

#[derive(Debug)]
pub struct PgBuilder {
    pool: PgPool,
    default_srid: Option<i32>,
    disable_bounds: bool,
    max_feature_count: Option<usize>,
    auto_functions: Option<PgBuilderPublish>,
    auto_tables: Option<PgBuilderPublish>,
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

    pub async fn instantiate_tables(&self) -> Result<(Sources, TableInfoSources)> {
        let mut found_pg_tables = query_available_tables(&self.pool).await?;

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

            let Some(found_schemas) = find_info(&found_pg_tables, &cfg_inf.schema, "schema", id) else { continue };
            let Some(found_tables) = find_info(found_schemas, &cfg_inf.table, "table", id) else { continue };
            let Some(found_inf) = find_info(found_tables, &cfg_inf.geometry_column, "geometry column", id) else { continue };

            let dup = !used.insert((&cfg_inf.schema, &cfg_inf.table, &cfg_inf.geometry_column));
            let dup = if dup { "duplicate " } else { "" };

            let id2 = self.resolve_id(id, cfg_inf);
            let Some(cfg_inf) = merge_table_info(self.default_srid, &id2, cfg_inf, found_inf) else { continue };
            warn_on_rename(id, &id2, "Table");
            info!("Configured {dup}source {id2} from {}", summary(&cfg_inf));
            pending.push(table_to_query(
                id2,
                cfg_inf,
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
                .unwrap_or_else(|| found_pg_tables.keys().cloned().collect());
            for schema in schemas.iter().sorted() {
                let Some(schema) = normalize_key(&found_pg_tables, schema, "schema", "") else { continue };
                let found_tables = found_pg_tables.remove(&schema).unwrap();
                for (table, geoms) in found_tables.into_iter().sorted_by(by_key) {
                    for (column, mut found_tbl) in geoms.into_iter().sorted_by(by_key) {
                        if used.contains(&(schema.as_str(), table.as_str(), column.as_str())) {
                            continue;
                        }
                        let source_id = auto_tables
                            .source_id_format
                            .replace("{schema}", &schema)
                            .replace("{table}", &table)
                            .replace("{column}", &column);
                        let id2 = self.resolve_id(&source_id, &found_tbl);
                        let Some(srid) = calc_srid(&found_tbl.format_id(), &id2, found_tbl.srid, 0, self.default_srid) else { continue };
                        found_tbl.srid = srid;
                        info!("Discovered source {id2} from {}", summary(&found_tbl));
                        pending.push(table_to_query(
                            id2,
                            found_tbl,
                            self.pool.clone(),
                            self.disable_bounds,
                            self.max_feature_count,
                        ));
                    }
                }
            }
        }

        let mut res = Sources::default();
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

    pub async fn instantiate_functions(&self) -> Result<(Sources, FuncInfoSources)> {
        let mut found_pg_funcs = query_available_function(&self.pool).await?;
        let mut res = Sources::default();
        let mut info_map = FuncInfoSources::new();
        let mut used = HashSet::<(&str, &str)>::new();

        for (id, cfg_inf) in &self.functions {
            let Some(schemas) = find_info(&found_pg_funcs, &cfg_inf.schema, "schema", id) else { continue };
            if schemas.is_empty() {
                warn!("No functions found in schema {}. Only functions like (z,x,y) -> bytea and similar are considered. See README.md", cfg_inf.schema);
                continue;
            }
            let Some((pg_sql, _)) = find_info(schemas, &cfg_inf.function, "function", id) else { continue };

            let dup = !used.insert((&cfg_inf.schema, &cfg_inf.function));
            let dup = if dup { "duplicate " } else { "" };

            let id2 = self.resolve_id(id, cfg_inf);
            self.add_func_src(&mut res, id2.clone(), cfg_inf, pg_sql.clone());
            warn_on_rename(id, &id2, "Function");
            let signature = &pg_sql.signature;
            info!("Configured {dup}source {id2} from the function {signature}");
            debug!("{}", pg_sql.query);
            info_map.insert(id2, cfg_inf.clone());
        }

        // Sort the discovered sources by schema and function name to ensure a consistent behavior
        if let Some(auto_funcs) = &self.auto_functions {
            let schemas = auto_funcs
                .schemas
                .as_ref()
                .cloned()
                .unwrap_or_else(|| found_pg_funcs.keys().cloned().collect());

            for schema in schemas.iter().sorted() {
                let Some(schema) = normalize_key(&found_pg_funcs, schema, "schema", "") else { continue; };
                let found_funcs = found_pg_funcs.remove(&schema).unwrap();
                for (func, (pg_sql, src_inf)) in found_funcs.into_iter().sorted_by(by_key) {
                    if used.contains(&(schema.as_str(), func.as_str())) {
                        continue;
                    }
                    let source_id = auto_funcs
                        .source_id_format
                        .replace("{schema}", &schema)
                        .replace("{function}", &func);
                    let id2 = self.resolve_id(&source_id, &src_inf);
                    self.add_func_src(&mut res, id2.clone(), &src_inf, pg_sql.clone());
                    info!("Discovered source {id2} from function {}", pg_sql.signature);
                    debug!("{}", pg_sql.query);
                    info_map.insert(id2, src_inf);
                }
            }
        }
        Ok((res, info_map))
    }

    fn resolve_id<T: PgInfo>(&self, id: &str, src_inf: &T) -> String {
        let signature = format!("{}.{}", self.pool.get_id(), src_inf.format_id());
        self.id_resolver.resolve(id, signature)
    }

    fn add_func_src(&self, sources: &mut Sources, id: String, info: &impl PgInfo, sql: PgSqlInfo) {
        let source = PgSource::new(
            id.clone(),
            sql,
            info.to_tilejson(id.clone()),
            self.pool.clone(),
        );
        sources.insert(id, Box::new(source));
    }
}

fn new_auto_publish(config: &PgConfig, is_function: bool) -> Option<PgBuilderPublish> {
    let default = |schemas| Some(PgBuilderPublish::new(is_function, None, schemas));

    if let Some(bo_a) = &config.auto_publish {
        match bo_a {
            BoolOrObject::Object(a) => match if is_function { &a.functions } else { &a.tables } {
                Some(bo_i) => match bo_i {
                    BoolOrObject::Object(item) => Some(PgBuilderPublish::new(
                        is_function,
                        item.source_id_format.as_ref(),
                        merge_opt_hs(&a.from_schemas, &item.from_schemas),
                    )),
                    BoolOrObject::Bool(true) => default(merge_opt_hs(&a.from_schemas, &None)),
                    BoolOrObject::Bool(false) => None,
                },
                // If auto_publish.functions is set, and currently asking for .tables which is missing,
                // .tables becomes the inverse of functions (i.e. an obj or true in tables means false in functions)
                None => match if is_function { &a.tables } else { &a.functions } {
                    Some(bo_i) => match bo_i {
                        BoolOrObject::Object(_) | BoolOrObject::Bool(true) => None,
                        BoolOrObject::Bool(false) => default(merge_opt_hs(&a.from_schemas, &None)),
                    },
                    None => default(merge_opt_hs(&a.from_schemas, &None)),
                },
            },
            BoolOrObject::Bool(true) => default(None),
            BoolOrObject::Bool(false) => None,
        }
    } else if config.tables.is_some() || config.functions.is_some() {
        None
    } else {
        default(None)
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
fn merge_opt_hs(
    a: &Option<OneOrMany<String>>,
    b: &Option<OneOrMany<String>>,
) -> Option<HashSet<String>> {
    if let Some(a) = a {
        let mut res: HashSet<_> = a.iter().cloned().collect();
        if let Some(b) = b {
            res.extend(b.iter().cloned());
        }
        Some(res)
    } else {
        b.as_ref().map(|b| b.iter().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    #[allow(clippy::unnecessary_wraps)]
    fn builder(source_id_format: &str, schemas: Option<&[&str]>) -> Option<PgBuilderPublish> {
        Some(PgBuilderPublish {
            source_id_format: source_id_format.to_string(),
            schemas: schemas.map(|s| s.iter().map(|s| (*s).to_string()).collect()),
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
