use std::io;
use std::str::FromStr;

use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use r2d2_postgres::PostgresConnectionManager;
use semver::Version;
use semver::VersionReq;

use crate::utils::prettify_error;

pub type Pool = r2d2::Pool<PostgresConnectionManager<MakeTlsConnector>>;
pub type Connection = r2d2::PooledConnection<PostgresConnectionManager<MakeTlsConnector>>;

fn make_tls_connector() -> io::Result<MakeTlsConnector> {
    let connector = TlsConnector::new().map_err(prettify_error("Can't build TLS connection"))?;
    let tls_connector = MakeTlsConnector::new(connector);
    Ok(tls_connector)
}

pub fn setup_connection_pool(cn_str: &str, pool_size: Option<u32>) -> io::Result<Pool> {
    let tls_connector = make_tls_connector()?;

    let config = postgres::config::Config::from_str(cn_str)
        .map_err(prettify_error("Can't parse connection string"))?;

    let manager = PostgresConnectionManager::new(config, tls_connector);

    let pool = r2d2::Pool::builder()
        .max_size(pool_size.unwrap_or(20))
        .build(manager)
        .map_err(prettify_error("Can't build connection pool"))?;

    Ok(pool)
}

pub fn get_connection(pool: &Pool) -> io::Result<Connection> {
    let connection = pool
        .get()
        .map_err(prettify_error("Can't retrieve connection from the pool"))?;

    Ok(connection)
}

pub fn select_postgis_verion(pool: &Pool) -> io::Result<String> {
    let mut connection = get_connection(pool)?;

    let version = connection
        .query_one(include_str!("scripts/get_postgis_version.sql"), &[])
        .map(|row| row.get::<_, String>("postgis_version"))
        .map_err(prettify_error("Can't get PostGIS version"))?;

    Ok(version)
}

pub fn check_postgis_version(required_postgis_version: &str, pool: &Pool) -> io::Result<bool> {
    let postgis_version = select_postgis_verion(&pool)?;

    let req = VersionReq::parse(required_postgis_version)
        .map_err(prettify_error("Can't parse required PostGIS version"))?;

    let version = Version::parse(postgis_version.as_str())
        .map_err(prettify_error("Can't parse database PostGIS version"))?;

    let matches = req.matches(&version);

    if !matches {
        error!(
            "Martin requires PostGIS {}, current version is {}",
            required_postgis_version, postgis_version
        );
    }

    Ok(matches)
}
