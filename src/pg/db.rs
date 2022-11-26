use crate::pg::config::{
    FormatId, FuncInfoDbMapMap, FuncInfoDbSources, FuncInfoSources, FunctionInfoDbInfo, PgConfig,
    PgConfigDb, TableInfo, TableInfoSources,
};
use crate::pg::function_source::{get_function_sources, FunctionSource};
use crate::pg::table_source::{get_table_sources, TableSource};
use crate::pg::utils::io_error;
use crate::source::IdResolver;
use crate::srv::server::Sources;
use bb8::PooledConnection;
use bb8_postgres::{tokio_postgres as pg, PostgresConnectionManager};
use futures::future::try_join;
use log::{debug, info, warn};
#[cfg(feature = "ssl")]
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
#[cfg(feature = "ssl")]
use postgres_openssl::MakeTlsConnector;
use semver::{Version, VersionReq};
use std::collections::HashMap;
use std::io;
use std::str::FromStr;

#[cfg(feature = "ssl")]
pub type ConnectionManager = PostgresConnectionManager<MakeTlsConnector>;
#[cfg(not(feature = "ssl"))]
pub type ConnectionManager = PostgresConnectionManager<postgres::NoTls>;

pub type Pool = bb8::Pool<ConnectionManager>;
pub type Connection<'a> = PooledConnection<'a, ConnectionManager>;

// We require ST_TileEnvelope that was added in PostGIS 3.0.0
const REQUIRED_POSTGIS_VERSION: &str = ">= 3.0.0";

pub async fn resolve_pg_data(
    config: PgConfig,
    id_resolver: IdResolver,
) -> io::Result<(Sources, PgConfigDb, Pool)> {
    let pg = PgConfigurator::new(&config, id_resolver).await?;
    let ((mut tables, tbl_info), (funcs, func_info)) =
        try_join(pg.instantiate_tables(), pg.instantiate_functions()).await?;
    tables.extend(funcs);
    Ok((tables, config.to_db(tbl_info, func_info), pg.pool))
}

/// A test helper to configure and instantiate a Postgres connection pool
pub async fn make_pool(config: PgConfig) -> io::Result<Pool> {
    Ok(PgConfigurator::new(&config, IdResolver::default())
        .await?
        .pool)
}

struct PgConfigurator {
    pub pool: Pool,
    default_srid: Option<i32>,
    discover_functions: bool,
    discover_tables: bool,
    id_resolver: IdResolver,
    db_id: String,
    table_sources: TableInfoSources,
    function_sources: FuncInfoSources,
}

impl PgConfigurator {
    async fn new(config: &PgConfig, id_resolver: IdResolver) -> io::Result<Self> {
        let conn_str = config.connection_string.as_str();
        info!("Connecting to {conn_str}");
        let pg_cfg = pg::config::Config::from_str(conn_str)
            .map_err(|e| io_error!(e, "Can't parse connection string {conn_str}"))?;

        let db_id = pg_cfg
            .get_dbname()
            .map_or_else(|| format!("{:?}", pg_cfg.get_hosts()[0]), |v| v.to_string());

        #[cfg(not(feature = "ssl"))]
        let manager = ConnectionManager::new(pg_cfg, postgres::NoTls);

        #[cfg(feature = "ssl")]
        let manager = {
            let mut builder = SslConnector::builder(SslMethod::tls())
                .map_err(|e| io_error!(e, "Can't build TLS connection"))?;

            if config.danger_accept_invalid_certs {
                builder.set_verify(SslVerifyMode::NONE);
            }

            if let Some(ca_root_file) = &config.ca_root_file {
                info!("Using {ca_root_file} as trusted root certificate");
                builder.set_ca_file(ca_root_file).map_err(|e| {
                    io_error!(e, "Can't set trusted root certificate {ca_root_file}")
                })?;
            }
            PostgresConnectionManager::new(pg_cfg, MakeTlsConnector::new(builder.build()))
        };

        let pool = Pool::builder()
            .max_size(config.pool_size)
            .build(manager)
            .await
            .map_err(|e| io_error!(e, "Can't build connection pool"))?;

        let postgis_version = select_postgis_version(&pool).await?;
        let req = VersionReq::parse(REQUIRED_POSTGIS_VERSION)
            .map_err(|e| io_error!(e, "Can't parse required PostGIS version"))?;
        let version = Version::parse(postgis_version.as_str())
            .map_err(|e| io_error!(e, "Can't parse database PostGIS version"))?;

        if !req.matches(&version) {
            Err(io::Error::new(io::ErrorKind::Other, format!("Martin requires PostGIS {REQUIRED_POSTGIS_VERSION}, current version is {postgis_version}")))
        } else {
            Ok(Self {
                pool,
                default_srid: config.default_srid,
                discover_functions: config.discover_functions,
                discover_tables: config.discover_tables,
                id_resolver,
                db_id,
                table_sources: config.table_sources.clone(),
                function_sources: config.function_sources.clone(),
            })
        }
    }

    async fn instantiate_tables(&self) -> Result<(Sources, TableInfoSources), io::Error> {
        let mut sources: Sources = HashMap::new();
        for (id, info) in &self.table_sources {
            self.add_table_src(&mut sources, None, id.clone(), info.clone());
        }
        let mut tables = TableInfoSources::new();
        if self.discover_tables {
            info!("Automatically detecting table sources");
            let srcs = get_table_sources(&self.pool, &self.table_sources, self.default_srid).await;
            for info in srcs? {
                self.add_table_src(&mut sources, Some(&mut tables), info.table.clone(), info);
            }
        }
        Ok((sources, tables))
    }

    fn add_table_src(
        &self,
        sources: &mut Sources,
        tables: Option<&mut TableInfoSources>,
        id: String,
        info: TableInfo,
    ) {
        let unique_id = info.format_id(&self.db_id);
        let id = self.id_resolver.resolve(id, unique_id);
        let prefix = if let Some(tables) = tables {
            tables.insert(id.clone(), info.clone());
            "Discovered"
        } else {
            "Configured"
        };
        info!(
            r#"{prefix} table source "{id}" from "{}.{}" with "{}" column ({}, SRID={})"#,
            info.schema,
            info.table,
            info.geometry_column,
            info.geometry_type.as_deref().unwrap_or("null"),
            info.srid
        );
        let source = TableSource::new(id.clone(), info, self.pool.clone());
        sources.insert(id, Box::new(source));
    }

    async fn instantiate_functions(&self) -> Result<(Sources, FuncInfoDbSources), io::Error> {
        let mut res: Sources = HashMap::new();
        let mut func_info = FuncInfoDbSources::new();
        let mut sources = get_function_sources(&self.pool).await?;
        let mut used_srcs = FuncInfoDbMapMap::new();

        for (id, info) in &self.function_sources {
            let schema = info.schema.as_str();
            let name = info.function.as_str();
            if let Some(db_inf) = sources.get_mut(schema).and_then(|v| v.remove(name)) {
                let db_inf = db_inf.with_info(info);
                let id = self.add_func_src(&mut res, id.clone(), db_inf.clone());
                let sig = &db_inf.signature;
                info!("Configured function source {id} -> {sig}");
                debug!("{}", db_inf.query);
                func_info.insert(id, db_inf.clone());
                // Store it just in case another source needs the same function
                used_srcs
                    .entry(schema.to_string())
                    .or_default()
                    .insert(name.to_string(), db_inf);
            } else if let Some(db_inf) = used_srcs.get_mut(schema).and_then(|v| v.get(name)) {
                // This function was already used in another source
                let db_inf = db_inf.with_info(info);
                let id = self.add_func_src(&mut res, id.clone(), db_inf.clone());
                let sig = &db_inf.signature;
                info!("Configured duplicate function source {id} -> {sig}");
                debug!("{}", db_inf.query);
                func_info.insert(id, db_inf);
            } else {
                warn!(
                    "Configured function source {id} from {schema}.{name} doesn't exist or \
                    doesn't have an expected signature like (z,x,y) -> bytea. See README.md",
                );
            }
        }

        if self.discover_functions {
            for funcs in sources.into_values() {
                for (name, db_inf) in funcs {
                    let id = self.add_func_src(&mut res, name, db_inf.clone());
                    let sig = &db_inf.signature;
                    info!("Discovered function source {id} -> {sig}");
                    debug!("{}", db_inf.query);
                    func_info.insert(id, db_inf);
                }
            }
        }

        Ok((res, func_info))
    }

    fn add_func_src(&self, sources: &mut Sources, id: String, info: FunctionInfoDbInfo) -> String {
        let resolver = &self.id_resolver;
        let id = resolver.resolve(id, info.info.format_id(&self.db_id));
        let source = FunctionSource::new(id.clone(), info, self.pool.clone());
        sources.insert(id.clone(), Box::new(source));
        id
    }
}

pub async fn get_connection(pool: &Pool) -> io::Result<Connection<'_>> {
    pool.get()
        .await
        .map_err(|e| io_error!(e, "Can't retrieve connection from the pool"))
}

async fn select_postgis_version(pool: &Pool) -> io::Result<String> {
    get_connection(pool)
        .await?
        .query_one(include_str!("scripts/get_postgis_version.sql"), &[])
        .await
        .map(|row| row.get::<_, String>("postgis_version"))
        .map_err(|e| io_error!(e, "Can't get PostGIS version"))
}
