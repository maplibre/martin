use crate::pg::config::{
    FuncInfoSources, FunctionInfo, InfoMap, PgConfig, PgInfo, TableInfo, TableInfoSources,
};
use crate::pg::connection::Pool;
use crate::pg::function_source::get_function_sources;
use crate::pg::pg_source::{PgSource, PgSqlInfo};
use crate::pg::table_source::{get_table_sources, table_to_query};
use crate::source::IdResolver;
use crate::srv::server::Sources;
use futures::future::{join_all, try_join};
use itertools::Itertools;
use log::{debug, error, info, warn};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::io;

pub async fn resolve_pg_data(
    config: PgConfig,
    id_resolver: IdResolver,
) -> io::Result<(Sources, PgConfig, Pool)> {
    let pg = PgBuilder::new(&config, id_resolver).await?;
    let ((mut tables, tbl_info), (funcs, func_info)) =
        try_join(pg.instantiate_tables(), pg.instantiate_functions()).await?;

    tables.extend(funcs);
    Ok((
        tables,
        PgConfig {
            tables: tbl_info,
            functions: func_info,
            ..config
        },
        pg.pool,
    ))
}

struct PgBuilder {
    pool: Pool,
    default_srid: Option<i32>,
    discover_functions: bool,
    discover_tables: bool,
    id_resolver: IdResolver,
    tables: TableInfoSources,
    functions: FuncInfoSources,
}

impl PgBuilder {
    async fn new(config: &PgConfig, id_resolver: IdResolver) -> io::Result<Self> {
        let pool = Pool::new(config).await?;
        Ok(Self {
            pool,
            default_srid: config.default_srid,
            discover_functions: config.discover_functions,
            discover_tables: config.discover_tables,
            id_resolver,
            tables: config.tables.clone(),
            functions: config.functions.clone(),
        })
    }

    pub async fn instantiate_tables(&self) -> Result<(Sources, TableInfoSources), io::Error> {
        let mut info_map = TableInfoSources::new();
        let mut discovered_sources = get_table_sources(&self.pool).await?;
        let mut used = SqlTableInfoMapMapMap::new();
        let mut pending = Vec::new();

        // First match configured sources with the discovered ones and add them to the pending list.
        // Note that multiple configured sources could map to a single discovered one.
        // After that, add the remaining discovered sources to the pending list if auto-config is enabled.
        for (id, cfg_inf) in &self.tables {
            if let Some(src_inf) = discovered_sources
                .get_mut(&cfg_inf.schema)
                .and_then(|v| v.get_mut(&cfg_inf.table))
                .and_then(|v| v.remove(&cfg_inf.geometry_column))
            {
                // Store it just in case another source needs the same table
                used.entry(src_inf.schema.to_string())
                    .or_default()
                    .entry(src_inf.table.to_string())
                    .or_default()
                    .insert(src_inf.geometry_column.to_string(), src_inf.clone());

                let id2 = self.resolve_id(id.clone(), cfg_inf);
                let Some(cfg_inf) = self.merge_info(&id2, cfg_inf, &src_inf) else {continue};
                warn_on_rename(id, &id2, "table");
                info!("Configured source {id2} from {}", summary(&cfg_inf));
                pending.push(table_to_query(id2, cfg_inf, self.pool.clone()));
            } else if let Some(src_inf) = used
                .get_mut(&cfg_inf.schema)
                .and_then(|v| v.get(&cfg_inf.table))
                .and_then(|v| v.get(&cfg_inf.geometry_column))
            {
                // This table was already used by another source
                let id2 = self.resolve_id(id.clone(), cfg_inf);
                let Some(cfg_inf) = self.merge_info(&id2, cfg_inf, src_inf) else {continue};
                warn_on_rename(id, &id2, "table");
                let info = summary(&cfg_inf);
                info!("Configured duplicate source {id2} from {info}");
                pending.push(table_to_query(id2, cfg_inf, self.pool.clone()));
            } else {
                warn!(
                    "Configured table source {id} as {}.{} with geo column {} does not exist",
                    cfg_inf.schema, cfg_inf.table, cfg_inf.geometry_column
                );
            }
        }

        if self.discover_tables {
            // Sort the discovered sources by schema, table and geometry column to ensure a consistent behavior
            for (_, tables) in discovered_sources.into_iter().sorted_by(by_key) {
                for (_, geoms) in tables.into_iter().sorted_by(by_key) {
                    for (_, src_inf) in geoms.into_iter().sorted_by(by_key) {
                        let id2 = self.resolve_id(src_inf.table.clone(), &src_inf);
                        let Some(cfg_inf) = self.merge_info(&id2, &src_inf, &src_inf) else {continue};
                        info!("Discovered source {id2} from {}", summary(&cfg_inf));
                        pending.push(table_to_query(id2, cfg_inf, self.pool.clone()));
                    }
                }
            }
        }

        let mut res: Sources = HashMap::new();
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

    pub async fn instantiate_functions(&self) -> Result<(Sources, FuncInfoSources), io::Error> {
        let mut discovered_sources = get_function_sources(&self.pool).await?;
        let mut res: Sources = HashMap::new();
        let mut info_map = FuncInfoSources::new();
        let mut used: HashMap<String, HashMap<String, PgSqlInfo>> = HashMap::new();

        for (id, cfg_inf) in &self.functions {
            let schema = &cfg_inf.schema;
            let name = &cfg_inf.function;
            if let Some((pg_sql, _)) = discovered_sources
                .get_mut(schema)
                .and_then(|v| v.remove(name))
            {
                // Store it just in case another source needs the same function
                used.entry(schema.to_string())
                    .or_default()
                    .insert(name.to_string(), pg_sql.clone());

                let id2 = self.resolve_id(id.clone(), cfg_inf);
                self.add_func_src(&mut res, id2.clone(), cfg_inf, pg_sql.clone());
                warn_on_rename(id, &id2, "function");
                info!("Configured source {id2} from function {}", pg_sql.signature);
                debug!("{}", pg_sql.query);
                info_map.insert(id2, cfg_inf.clone());
            } else if let Some(pg_sql) = used.get_mut(schema).and_then(|v| v.get(name)) {
                // This function was already used by another source
                let id2 = self.resolve_id(id.clone(), cfg_inf);
                self.add_func_src(&mut res, id2.clone(), cfg_inf, pg_sql.clone());
                warn_on_rename(id, &id2, "function");
                let sig = &pg_sql.signature;
                info!("Configured duplicate source {id2} from function {sig}");
                debug!("{}", pg_sql.query);
                info_map.insert(id2, cfg_inf.clone());
            } else {
                warn!(
                    "Configured function source {id} from {schema}.{name} does not exist or \
                    does not have an expected signature like (z,x,y) -> bytea. See README.md",
                );
            }
        }

        if self.discover_functions {
            // Sort the discovered sources by schema and function name to ensure a consistent behavior
            for (_, funcs) in discovered_sources.into_iter().sorted_by(by_key) {
                for (name, (pg_sql, src_inf)) in funcs.into_iter().sorted_by(by_key) {
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

    fn add_func_src(&self, sources: &mut Sources, id: String, info: &impl PgInfo, sql: PgSqlInfo) {
        let source = PgSource::new(id.clone(), sql, info.to_tilejson(), self.pool.clone());
        sources.insert(id, Box::new(source));
    }

    fn merge_info(
        &self,
        new_id: &String,
        cfg_inf: &TableInfo,
        src_inf: &TableInfo,
    ) -> Option<TableInfo> {
        // Assume cfg_inf and src_inf have the same schema/table/geometry_column
        let table_id = src_inf.format_id();
        let mut inf = cfg_inf.clone();
        inf.srid = match (src_inf.srid, cfg_inf.srid, self.default_srid) {
            (0, 0, Some(default_srid)) => {
                info!("Table {table_id} has SRID=0, using provided default SRID={default_srid}");
                default_srid
            }
            (0, 0, None) => {
                let info = "To use this table source, set default or specify this table SRID in the config file, or set the default SRID with  --default-srid=...";
                warn!("Table {table_id} has SRID=0, skipping. {info}");
                return None;
            }
            (0, cfg, _) => cfg, // Use the configured SRID
            (src, 0, _) => src, // Use the source SRID
            (src, cfg, _) if src != cfg => {
                warn!("Table {table_id} has SRID={src}, but source {new_id} has SRID={cfg}");
                return None;
            }
            (_, cfg, _) => cfg,
        };

        match (&src_inf.geometry_type, &cfg_inf.geometry_type) {
            (Some(src), Some(cfg)) if src != cfg => {
                warn!(r#"Table {table_id} has geometry type={src}, but source {new_id} has {cfg}"#);
            }
            _ => {}
        }

        Some(inf)
    }
}

fn warn_on_rename(old_id: &String, new_id: &String, typ: &str) {
    if old_id != new_id {
        warn!("Configured {typ} source {old_id} was renamed to {new_id} due to ID conflict");
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

pub type SqlFuncInfoMapMap = InfoMap<InfoMap<(PgSqlInfo, FunctionInfo)>>;
pub type SqlTableInfoMapMapMap = InfoMap<InfoMap<InfoMap<TableInfo>>>;
