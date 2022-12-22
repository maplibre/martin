use crate::pg::config::{PgConfig, PgInfo};
use crate::pg::config_function::{FuncInfoSources, FunctionInfo};
use crate::pg::config_table::{TableInfo, TableInfoSources};
use crate::pg::function_source::get_function_sources;
use crate::pg::pg_source::{PgSource, PgSqlInfo};
use crate::pg::pool::Pool;
use crate::pg::table_source::{calc_srid, get_table_sources, merge_table_info, table_to_query};
use crate::pg::utils::PgError::InvalidTableExtent;
use crate::pg::utils::Result;
use crate::source::IdResolver;
use crate::srv::server::Sources;
use crate::utils::{find_info, normalize_key, InfoMap, Schemas};
use futures::future::join_all;
use itertools::Itertools;
use log::{debug, error, info, warn};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

pub type SqlFuncInfoMapMap = InfoMap<InfoMap<(PgSqlInfo, FunctionInfo)>>;
pub type SqlTableInfoMapMapMap = InfoMap<InfoMap<InfoMap<TableInfo>>>;

pub struct PgBuilder {
    pool: Pool,
    default_srid: Option<i32>,
    auto_functions: Schemas,
    auto_tables: Schemas,
    id_resolver: IdResolver,
    tables: TableInfoSources,
    functions: FuncInfoSources,
}

impl PgBuilder {
    pub async fn new(config: &PgConfig, id_resolver: IdResolver) -> Result<Self> {
        let pool = Pool::new(config).await?;
        let auto = config.run_autodiscovery;
        Ok(Self {
            pool,
            default_srid: config.default_srid,
            auto_functions: config.auto_functions.clone().unwrap_or(Schemas::Bool(auto)),
            auto_tables: config.auto_tables.clone().unwrap_or(Schemas::Bool(auto)),
            id_resolver,
            tables: config.tables.clone().unwrap_or_default(),
            functions: config.functions.clone().unwrap_or_default(),
        })
    }

    pub async fn instantiate_tables(&self) -> Result<(Sources, TableInfoSources)> {
        let mut all_tables = get_table_sources(&self.pool).await?;

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

            let Some(schemas) = find_info(&all_tables, &cfg_inf.schema, "schema", id) else { continue };
            let Some(tables) = find_info(schemas, &cfg_inf.table, "table", id) else { continue };
            let Some(src_inf) = find_info(tables, &cfg_inf.geometry_column, "geometry column", id) else { continue };

            let dup = used.insert((&cfg_inf.schema, &cfg_inf.table, &cfg_inf.geometry_column));
            let dup = if dup { "duplicate " } else { "" };

            let id2 = self.resolve_id(id.clone(), cfg_inf);
            let Some(cfg_inf) = merge_table_info(self.default_srid,&id2, cfg_inf, src_inf) else { continue };
            warn_on_rename(id, &id2, "Table");
            info!("Configured {dup}source {id2} from {}", summary(&cfg_inf));
            pending.push(table_to_query(id2, cfg_inf, self.pool.clone()));
        }

        // Sort the discovered sources by schema, table and geometry column to ensure a consistent behavior
        for schema in self.auto_tables.get(|| all_tables.keys()) {
            let Some(schema2) = normalize_key(&all_tables, &schema, "schema", "") else { continue };
            let tables = all_tables.remove(&schema2).unwrap();
            for (table, geoms) in tables.into_iter().sorted_by(by_key) {
                for (geom, mut src_inf) in geoms.into_iter().sorted_by(by_key) {
                    if used.contains(&(schema.as_str(), table.as_str(), geom.as_str())) {
                        continue;
                    }
                    let id2 = self.resolve_id(table.clone(), &src_inf);
                    let Some(srid) = calc_srid(&src_inf.format_id(), &id2,  src_inf.srid,0, self.default_srid) else {continue};
                    src_inf.srid = srid;
                    info!("Discovered source {id2} from {}", summary(&src_inf));
                    pending.push(table_to_query(id2, src_inf, self.pool.clone()));
                }
            }
        }

        let mut res: Sources = HashMap::new();
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
        let mut all_funcs = get_function_sources(&self.pool).await?;
        let mut res: Sources = HashMap::new();
        let mut info_map = FuncInfoSources::new();
        let mut used = HashSet::<(&str, &str)>::new();

        for (id, cfg_inf) in &self.functions {
            let Some(schemas) = find_info(&all_funcs, &cfg_inf.schema, "schema", id) else { continue };
            if schemas.is_empty() {
                warn!("No functions found in schema {}. Only functions like (z,x,y) -> bytea and similar are considered. See README.md", cfg_inf.schema);
                continue;
            }
            let Some((pg_sql, _)) = find_info(schemas, &cfg_inf.function, "function", id) else { continue };

            let dup = !used.insert((&cfg_inf.schema, &cfg_inf.function));
            let dup = if dup { "duplicate " } else { "" };

            let id2 = self.resolve_id(id.clone(), cfg_inf);
            self.add_func_src(&mut res, id2.clone(), cfg_inf, pg_sql.clone());
            warn_on_rename(id, &id2, "Function");
            let signature = &pg_sql.signature;
            info!("Configured {dup}source {id2} from the function {signature}");
            debug!("{}", pg_sql.query);
            info_map.insert(id2, cfg_inf.clone());
        }

        // Sort the discovered sources by schema and function name to ensure a consistent behavior
        for schema in self.auto_functions.get(|| all_funcs.keys()) {
            let Some(schema2) = normalize_key(&all_funcs, &schema, "schema", "") else { continue };
            let funcs = all_funcs.remove(&schema2).unwrap();
            for (name, (pg_sql, src_inf)) in funcs.into_iter().sorted_by(by_key) {
                if used.contains(&(schema.as_str(), name.as_str())) {
                    continue;
                }
                let id2 = self.resolve_id(name.clone(), &src_inf);
                self.add_func_src(&mut res, id2.clone(), &src_inf, pg_sql.clone());
                info!("Discovered source {id2} from function {}", pg_sql.signature);
                debug!("{}", pg_sql.query);
                info_map.insert(id2, src_inf);
            }
        }

        Ok((res, info_map))
    }

    fn resolve_id<T: PgInfo>(&self, id: String, src_inf: &T) -> String {
        let signature = format!("{}.{}", self.pool.get_id(), src_inf.format_id());
        self.id_resolver.resolve(id, signature)
    }

    fn add_func_src(&self, sources: &mut Sources, id: String, info: &impl PgInfo, sql: PgSqlInfo) {
        let source = PgSource::new(id.clone(), sql, info.to_tilejson(), self.pool.clone());
        sources.insert(id, Box::new(source));
    }

    #[must_use]
    pub fn get_pool(self) -> Pool {
        self.pool
    }
}

fn warn_on_rename(old_id: &String, new_id: &String, typ: &str) {
    if old_id != new_id {
        warn!("{typ} source {old_id} was renamed to {new_id} due to ID conflict");
    }
}

fn summary(info: &TableInfo) -> String {
    format!(
        "table {}.{} with {} column ({}, SRID={})",
        info.schema,
        info.table,
        info.geometry_column,
        info.geometry_type
            .as_deref()
            .unwrap_or("UNKNOWN GEOMETRY TYPE"),
        info.srid,
    )
}

fn by_key<T>(a: &(String, T), b: &(String, T)) -> Ordering {
    a.0.cmp(&b.0)
}
