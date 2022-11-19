use crate::config::Config;
use crate::pg::config::PgConfig;
use crate::pg::function_source::get_function_sources;
use crate::pg::table_source::get_table_sources;
use crate::pg::utils::prettify_error;
use bb8::PooledConnection;
use bb8_postgres::{tokio_postgres as pg, PostgresConnectionManager};
use log::info;
#[cfg(feature = "ssl")]
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
#[cfg(feature = "ssl")]
use postgres_openssl::MakeTlsConnector;
use semver::{Version, VersionReq};
use std::io;
use std::str::FromStr;

#[cfg(feature = "ssl")]
pub type ConnectionManager = PostgresConnectionManager<MakeTlsConnector>;
#[cfg(not(feature = "ssl"))]
pub type ConnectionManager = PostgresConnectionManager<postgres::NoTls>;

pub type Pool = bb8::Pool<ConnectionManager>;
pub type Connection<'a> = PooledConnection<'a, ConnectionManager>;

const REQUIRED_POSTGIS_VERSION: &str = ">= 2.4.0";

pub async fn setup_connection_pool(config: &PgConfig) -> io::Result<Pool> {
    let cfg = pg::Config::from_str(config.connection_string.as_str())
        .map_err(|e| prettify_error!(e, "Can't parse connection string"))?;

    #[cfg(not(feature = "ssl"))]
    let mgr = ConnectionManager::new(cfg, postgres::NoTls);

    #[cfg(feature = "ssl")]
    let mgr = {
        let mut builder = SslConnector::builder(SslMethod::tls())
            .map_err(|e| prettify_error!(e, "Can't build TLS connection"))?;

        if config.danger_accept_invalid_certs {
            builder.set_verify(SslVerifyMode::NONE);
        }

        if let Some(ca_root_file) = &config.ca_root_file {
            info!("Using {ca_root_file} as trusted root certificate");
            builder.set_ca_file(ca_root_file).map_err(|e| {
                prettify_error!(e, "Can't set trusted root certificate {}", ca_root_file)
            })?;
        }

        ConnectionManager::new(cfg, MakeTlsConnector::new(builder.build()))
    };

    Pool::builder()
        .max_size(config.pool_size)
        .build(mgr)
        .await
        .map_err(|e| prettify_error!(e, "Can't build connection pool"))
}

pub async fn get_connection(pool: &Pool) -> io::Result<Connection<'_>> {
    pool.get()
        .await
        .map_err(|e| prettify_error!(e, "Can't retrieve connection from the pool"))
}

async fn select_postgis_version(pool: &Pool) -> io::Result<String> {
    let connection = get_connection(pool).await?;

    connection
        .query_one(include_str!("scripts/get_postgis_version.sql"), &[])
        .await
        .map(|row| row.get::<_, String>("postgis_version"))
        .map_err(|e| prettify_error!(e, "Can't get PostGIS version"))
}

async fn validate_postgis_version(pool: &Pool) -> io::Result<()> {
    let postgis_version = select_postgis_version(pool).await?;
    let req = VersionReq::parse(REQUIRED_POSTGIS_VERSION)
        .map_err(|e| prettify_error!(e, "Can't parse required PostGIS version"))?;
    let version = Version::parse(postgis_version.as_str())
        .map_err(|e| prettify_error!(e, "Can't parse database PostGIS version"))?;
    if req.matches(&version) {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::Other, format!("Martin requires PostGIS {REQUIRED_POSTGIS_VERSION}, current version is {postgis_version}")))
    }
}

pub async fn configure_db_sources(mut config: &mut Config) -> io::Result<Pool> {
    info!("Connecting to database");

    let pool = setup_connection_pool(&config.pg).await?;
    validate_postgis_version(&pool).await?;

    let info_prefix = if config.pg.use_dynamic_sources {
        info!("Automatically detecting table and function sources");
        let mut connection = get_connection(&pool).await?;

        let sources = get_table_sources(&mut connection, config.pg.default_srid).await?;
        if sources.is_empty() {
            info!("No table sources found");
        } else {
            config.pg.table_sources = sources;
        }

        let sources = get_function_sources(&mut connection).await?;
        if sources.is_empty() {
            info!("No function sources found");
        } else {
            config.pg.function_sources = sources;
        }

        "Found"
    } else {
        "Loaded"
    };

    for table_source in config.pg.table_sources.values() {
        info!(
            r#"{info_prefix} "{}" table source with "{}" column ({}, SRID={})"#,
            table_source.id,
            table_source.geometry_column,
            table_source.geometry_type.as_deref().unwrap_or("null"),
            table_source.srid
        );
    }
    for function_source in config.pg.function_sources.values() {
        info!("{info_prefix} {} function source", function_source.id);
    }
    Ok(pool)
}
