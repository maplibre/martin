use std::io;
use std::str::FromStr;

use crate::config::Config;
use crate::function_source::get_function_sources;
use crate::table_source::get_table_sources;
use bb8::PooledConnection;
use bb8_postgres::{tokio_postgres, PostgresConnectionManager};
use log::info;
use openssl::ssl::{SslConnector, SslMethod, SslVerifyMode};
use postgres_openssl::MakeTlsConnector;
use semver::{Version, VersionReq};

use crate::utils::prettify_error;

pub type ConnectionManager = PostgresConnectionManager<MakeTlsConnector>;
pub type Pool = bb8::Pool<ConnectionManager>;
pub type Connection<'a> = PooledConnection<'a, ConnectionManager>;

const REQUIRED_POSTGIS_VERSION: &str = ">= 2.4.0";

fn make_tls_connector(
    ca_root_file: &Option<String>,
    danger_accept_invalid_certs: bool,
) -> io::Result<MakeTlsConnector> {
    let mut builder = SslConnector::builder(SslMethod::tls())?;

    if danger_accept_invalid_certs {
        builder.set_verify(SslVerifyMode::NONE);
    }

    if let Some(ca_root_file) = ca_root_file {
        info!("Using {ca_root_file} as trusted root certificate");
        builder.set_ca_file(ca_root_file)?;
    }

    let tls_connector = MakeTlsConnector::new(builder.build());
    Ok(tls_connector)
}

pub async fn setup_connection_pool(
    connection_string: &str,
    ca_root_file: &Option<String>,
    pool_size: u32,
    danger_accept_invalid_certs: bool,
) -> io::Result<Pool> {
    let config = tokio_postgres::config::Config::from_str(connection_string)
        .map_err(|e| prettify_error!(e, "Can't parse connection string"))?;

    let tls_connector = make_tls_connector(ca_root_file, danger_accept_invalid_certs)
        .map_err(|e| prettify_error!(e, "Can't build TLS connection"))?;

    let manager = PostgresConnectionManager::new(config, tls_connector);

    let pool = Pool::builder()
        .max_size(pool_size)
        .build(manager)
        .await
        .map_err(|e| prettify_error!(e, "Can't build connection pool"))?;

    Ok(pool)
}

pub async fn get_connection(pool: &Pool) -> io::Result<Connection<'_>> {
    let connection = pool
        .get()
        .await
        .map_err(|e| prettify_error!(e, "Can't retrieve connection from the pool"))?;

    Ok(connection)
}

async fn select_postgis_version(pool: &Pool) -> io::Result<String> {
    let connection = get_connection(pool).await?;

    let version = connection
        .query_one(include_str!("scripts/get_postgis_version.sql"), &[])
        .await
        .map(|row| row.get::<_, String>("postgis_version"))
        .map_err(|e| prettify_error!(e, "Can't get PostGIS version"))?;

    Ok(version)
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

pub async fn configure_db_source(mut config: &mut Config) -> io::Result<Pool> {
    info!("Connecting to database");
    let pool = setup_connection_pool(
        &config.connection_string,
        &config.ca_root_file,
        config.pool_size,
        config.danger_accept_invalid_certs,
    )
    .await?;

    validate_postgis_version(&pool).await?;

    let info_prefix = if config.use_dynamic_sources {
        info!("Automatically detecting table and function sources");
        let mut connection = get_connection(&pool).await?;

        let sources = get_table_sources(&mut connection, config.default_srid).await?;
        if sources.is_empty() {
            info!("No table sources found");
        } else {
            config.table_sources = Some(sources);
        }

        let sources = get_function_sources(&mut connection).await?;
        if sources.is_empty() {
            info!("No function sources found");
        } else {
            config.function_sources = Some(sources);
        }

        "Found"
    } else {
        "Loaded"
    };

    if let Some(table_sources) = &config.table_sources {
        for table_source in table_sources.values() {
            info!(
                r#"{info_prefix} "{}" table source with "{}" column ({}, SRID={})"#,
                table_source.id,
                table_source.geometry_column,
                table_source.geometry_type.as_deref().unwrap_or("null"),
                table_source.srid
            );
        }
    }
    if let Some(function_sources) = &config.function_sources {
        for function_source in function_sources.values() {
            info!("{info_prefix} {} function source", function_source.id);
        }
    }
    Ok(pool)
}
