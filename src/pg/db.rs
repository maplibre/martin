use crate::pg::config::{
    FormatId, FunctionInfo, FunctionInfoSources, PgConfig, TableInfo, TableInfoSources,
};
use crate::pg::function_source::{get_function_sources, FunctionSource};
use crate::pg::table_source::{get_table_sources, TableSource};
use crate::pg::utils::io_error;
use crate::source::IdResolver;
use crate::srv::server::Sources;
use bb8::PooledConnection;
use bb8_postgres::{tokio_postgres as pg, PostgresConnectionManager};
use futures::future::try_join;
use log::info;
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

const REQUIRED_POSTGIS_VERSION: &str = ">= 2.4.0";

pub struct PgConfigurator {
    pool: Pool,
    default_srid: Option<i32>,
    discover_functions: bool,
    discover_tables: bool,
    id_resolver: IdResolver,
    db_id: String,
    pub table_sources: TableInfoSources,
    pub function_sources: FunctionInfoSources,
}

impl PgConfigurator {
    pub async fn new(config: &PgConfig, id_resolver: IdResolver) -> io::Result<Self> {
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

    pub async fn discover_db_sources(&mut self) -> io::Result<Sources> {
        let ((mut tables, tbl_info), (funcs, func_info)) =
            try_join(self.instantiate_tables(), self.instantiate_functions()).await?;
        self.table_sources.extend(tbl_info);
        self.function_sources.extend(func_info);
        tables.extend(funcs);
        Ok(tables)
    }

    pub fn get_pool(&self) -> &Pool {
        &self.pool
    }

    pub async fn instantiate_tables(&self) -> Result<(Sources, TableInfoSources), io::Error> {
        let mut sources: Sources = HashMap::new();
        for (id, info) in &self.table_sources {
            self.add_source(&mut sources, None, id.clone(), info.clone());
        }
        let mut tables = TableInfoSources::new();
        if self.discover_tables {
            info!("Automatically detecting table sources");
            for info in
                get_table_sources(&self.pool, &self.table_sources, self.default_srid).await?
            {
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
        sources.insert(
            id.clone(),
            Box::new(TableSource::new(id, info, self.pool.clone())),
        );
    }

    pub async fn instantiate_functions(&self) -> Result<(Sources, FunctionInfoSources), io::Error> {
        let mut sources: Sources = HashMap::new();
        for (id, info) in &self.function_sources {
            self.add_func_src(&mut sources, None, id.clone(), info.clone());
        }
        let mut funcs = FunctionInfoSources::new();
        if self.discover_functions {
            info!("Automatically detecting function sources");
            for info in get_function_sources(&self.pool, &self.function_sources).await? {
                self.add_func_src(&mut sources, Some(&mut funcs), info.function.clone(), info);
            }
        }
        Ok((sources, funcs))
    }

    fn add_func_src(
        &self,
        sources: &mut Sources,
        funcs: Option<&mut FunctionInfoSources>,
        id: String,
        info: FunctionInfo,
    ) {
        let id = self.id_resolver.resolve(id, info.format_id(&self.db_id));
        let prefix = if let Some(funcs) = funcs {
            funcs.insert(id.clone(), info.clone());
            "Discovered"
        } else {
            "Configured"
        };
        info!(
            r#"{prefix} function source "{id}" that calls {}.{}(...)"#,
            info.schema, info.function
        );
        sources.insert(
            id.clone(),
            Box::new(FunctionSource::new(id, info, self.pool.clone())),
        );
    }

    fn add_source(
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
        sources.insert(
            id.clone(),
            Box::new(TableSource::new(id, info, self.pool.clone())),
        );
    }
}

pub async fn get_connection(pool: &Pool) -> io::Result<Connection<'_>> {
    pool.get()
        .await
        .map_err(|e| io_error!(e, "Can't retrieve connection from the pool"))
}

async fn select_postgis_version(pool: &Pool) -> io::Result<String> {
    let connection = get_connection(pool).await?;

    connection
        .query_one(include_str!("scripts/get_postgis_version.sql"), &[])
        .await
        .map(|row| row.get::<_, String>("postgis_version"))
        .map_err(|e| io_error!(e, "Can't get PostGIS version"))
}
