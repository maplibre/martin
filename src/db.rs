use std::io;
use std::str::FromStr;

use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use r2d2::PooledConnection;
use r2d2_postgres::PostgresConnectionManager;
use semver::Version;
use semver::VersionReq;

use crate::utils::prettify_error;

pub type ConnectionManager = PostgresConnectionManager<MakeTlsConnector>;
pub type Pool = r2d2::Pool<ConnectionManager>;
pub type Connection = PooledConnection<ConnectionManager>;

fn make_tls_connector(danger_accept_invalid_certs: bool) -> io::Result<MakeTlsConnector> {
    let connector = TlsConnector::builder()
        .danger_accept_invalid_certs(danger_accept_invalid_certs)
        .build()
        .map_err(prettify_error("Can't build TLS connection".to_owned()))?;

    let tls_connector = MakeTlsConnector::new(connector);
    Ok(tls_connector)
}

pub fn setup_connection_pool(
    cn_str: &str,
    pool_size: Option<u32>,
    danger_accept_invalid_certs: bool,
) -> io::Result<Pool> {
    let config = postgres::config::Config::from_str(cn_str)
        .map_err(prettify_error("Can't parse connection string".to_owned()))?;

    let tls_connector = make_tls_connector(danger_accept_invalid_certs)?;
    let manager = PostgresConnectionManager::new(config, tls_connector);

    let pool = r2d2::Pool::builder()
        .max_size(pool_size.unwrap_or(20))
        .build(manager)
        .map_err(prettify_error("Can't build connection pool".to_owned()))?;

    Ok(pool)
}

pub fn get_connection(pool: &Pool) -> io::Result<Connection> {
    let connection = pool.get().map_err(prettify_error(
        "Can't retrieve connection from the pool".to_owned(),
    ))?;

    Ok(connection)
}

pub fn select_postgis_verion(pool: &Pool) -> io::Result<String> {
    let mut connection = get_connection(pool)?;

    let version = connection
        .query_one(include_str!("scripts/get_postgis_version.sql"), &[])
        .map(|row| row.get::<_, String>("postgis_version"))
        .map_err(prettify_error("Can't get PostGIS version".to_owned()))?;

    Ok(version)
}

pub fn check_postgis_version(required_postgis_version: &str, pool: &Pool) -> io::Result<bool> {
    let postgis_version = select_postgis_verion(pool)?;

    let req = VersionReq::parse(required_postgis_version).map_err(prettify_error(
        "Can't parse required PostGIS version".to_owned(),
    ))?;

    let version = Version::parse(postgis_version.as_str()).map_err(prettify_error(
        "Can't parse database PostGIS version".to_owned(),
    ))?;

    let matches = req.matches(&version);

    if !matches {
        error!(
            "Martin requires PostGIS {}, current version is {}",
            required_postgis_version, postgis_version
        );
    }

    Ok(matches)
}
