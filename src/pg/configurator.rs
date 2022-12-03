use crate::pg::config::FunctionInfo;
use crate::pg::config::TableInfo;
use crate::pg::config::{FuncInfoSources, InfoMap, PgConfig, PgInfo, PgSqlInfo, TableInfoSources};
use crate::pg::connection::Pool;
use crate::pg::function_source::get_function_sources;
use crate::pg::pg_source::PgSource;
use crate::pg::table_source::get_table_sources;
use crate::source::IdResolver;
use crate::srv::server::Sources;
use futures::future::try_join;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::io;

pub async fn resolve_pg_data(
    config: PgConfig,
    id_resolver: IdResolver,
) -> io::Result<(Sources, PgConfig, Pool)> {
    let pg = PgConfigurator::new(&config, id_resolver).await?;
    let ((mut tables, tbl_info), (funcs, func_info)) =
        try_join(pg.instantiate_tables(), pg.instantiate_functions()).await?;

    tables.extend(funcs);
    Ok((
        tables,
        PgConfig {
            table_sources: tbl_info,
            function_sources: func_info,
            ..config
        },
        pg.pool,
    ))
}

struct PgConfigurator {
    pool: Pool,
    default_srid: Option<i32>,
    discover_functions: bool,
    discover_tables: bool,
    id_resolver: IdResolver,
    table_sources: TableInfoSources,
    function_sources: FuncInfoSources,
}

impl PgConfigurator {
    async fn new(config: &PgConfig, id_resolver: IdResolver) -> io::Result<Self> {
        let pool = Pool::new(config).await?;
        Ok(Self {
            pool,
            default_srid: config.default_srid,
            discover_functions: config.discover_functions,
            discover_tables: config.discover_tables,
            id_resolver,
            table_sources: config.table_sources.clone(),
            function_sources: config.function_sources.clone(),
        })
    }

    async fn instantiate_tables(&self) -> Result<(Sources, InfoMap<TableInfo>), io::Error> {
        let mut res: Sources = HashMap::new();
        let mut info_map = InfoMap::new();
        let mut discovered_sources = get_table_sources(&self.pool, self.default_srid).await?;

        for (id, src_inf) in &self.table_sources {
            if let Some((pg_sql, _)) = discovered_sources
                .get_mut(&src_inf.schema)
                .and_then(|v| v.get_mut(&src_inf.table))
                .and_then(|v| v.remove(&src_inf.geometry_column))
            {
                let id2 = self.resolve_id(id.clone(), src_inf);
                self.add_func_src(&mut res, id2.clone(), src_inf, pg_sql.clone());
                warn_on_rename(id, &id2, "table");
                info!("Configured source {id2} from table {}", pg_sql.signature);
                debug!("{}", pg_sql.query);
                info_map.insert(id2, src_inf.clone());
            } else {
                warn!(
                    "Configured table source {id} as {}.{} with geo column {} does not exist",
                    src_inf.schema, src_inf.table, src_inf.geometry_column
                );
            }
        }

        if self.discover_tables {
            for tables in discovered_sources.into_values() {
                for geoms in tables.into_values() {
                    for (pg_sql, src_inf) in geoms.into_values() {
                        let table = &src_inf.table;
                        let id2 = self.resolve_id(table.clone(), &src_inf);
                        self.add_func_src(&mut res, id2.clone(), &src_inf, pg_sql.clone());
                        info!(
                            "Discovered source {id2} from table {}.{} with {} column ({}, SRID={})",
                            src_inf.schema,
                            src_inf.table,
                            src_inf.geometry_column,
                            src_inf.geometry_type.as_deref().unwrap_or("UNKNOWN"),
                            src_inf.srid,
                        );
                        debug!("{}", pg_sql.query);
                        info_map.insert(id2, src_inf);
                    }
                }
            }
        }

        Ok((res, info_map))
    }

    async fn instantiate_functions(&self) -> Result<(Sources, InfoMap<FunctionInfo>), io::Error> {
        let mut discovered_sources = get_function_sources(&self.pool).await?;
        let mut res: Sources = HashMap::new();
        let mut info_map = InfoMap::new();
        let mut used_funcs: HashMap<String, HashMap<String, PgSqlInfo>> = HashMap::new();

        for (id, src_inf) in &self.function_sources {
            let schema = &src_inf.schema;
            let name = &src_inf.function;
            if let Some((pg_sql, _)) = discovered_sources
                .get_mut(schema)
                .and_then(|v| v.remove(name))
            {
                let id2 = self.resolve_id(id.clone(), src_inf);
                self.add_func_src(&mut res, id2.clone(), src_inf, pg_sql.clone());
                warn_on_rename(id, &id2, "function");
                info!("Configured source {id2} from function {}", pg_sql.signature);
                debug!("{}", pg_sql.query);
                info_map.insert(id2, src_inf.clone());
                // Store it just in case another source needs the same function
                used_funcs
                    .entry(schema.to_string())
                    .or_default()
                    .insert(name.to_string(), pg_sql);
            } else if let Some(pg_sql) = used_funcs.get_mut(schema).and_then(|v| v.get(name)) {
                // This function was already used by another source
                let id2 = self.resolve_id(id.clone(), src_inf);
                self.add_func_src(&mut res, id2.clone(), src_inf, pg_sql.clone());
                warn_on_rename(id, &id2, "function");
                let sig = &pg_sql.signature;
                info!("Configured duplicate source {id2} from function {sig}");
                debug!("{}", pg_sql.query);
                info_map.insert(id2, src_inf.clone());
            } else {
                warn!(
                    "Configured function source {id} from {schema}.{name} does not exist or \
                    does not have an expected signature like (z,x,y) -> bytea. See README.md",
                );
            }
        }

        if self.discover_functions {
            for funcs in discovered_sources.into_values() {
                for (name, (pg_sql, src_inf)) in funcs {
                    let id2 = self.resolve_id(name.clone(), &src_inf);
                    self.add_func_src(&mut res, id2.clone(), &src_inf, pg_sql.clone());
                    info!("Discovered source {id2} from function {}", pg_sql.signature);
                    debug!("{}", pg_sql.query);
                    info_map.insert(id2, src_inf);
                }
            }
        }

        Ok((res, info_map))
    }

    fn resolve_id<T: PgInfo>(&self, id: String, src_inf: &T) -> String {
        let signature = format!("{}.{}", self.pool.get_id(), src_inf.format_id());
        self.id_resolver.resolve(id, signature)
    }

    fn add_func_src(
        &self,
        sources: &mut Sources,
        id: String,
        src_inf: &impl PgInfo,
        pg_sql: PgSqlInfo,
    ) {
        let source = PgSource::new(id.clone(), pg_sql, src_inf.to_tilejson(), self.pool.clone());
        sources.insert(id, Box::new(source));
    }
}

fn warn_on_rename(old_id: &String, new_id: &String, typ: &str) {
    if old_id != new_id {
        warn!("Configured {typ} source {old_id} was renamed to {new_id} due to ID conflict");
    }
}
