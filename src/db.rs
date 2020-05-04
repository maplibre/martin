use std::io;
use std::str::FromStr;

use postgres::NoTls;
use r2d2_postgres::PostgresConnectionManager;
use semver::Version;
use semver::VersionReq;

pub type Pool = r2d2::Pool<PostgresConnectionManager<NoTls>>;
pub type Connection = r2d2::PooledConnection<PostgresConnectionManager<NoTls>>;

pub fn setup_connection_pool(cn_str: &str, pool_size: Option<u32>) -> io::Result<Pool> {
  let config = postgres::config::Config::from_str(cn_str)
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  let manager = PostgresConnectionManager::new(config, NoTls);

  let pool = r2d2::Pool::builder()
    .max_size(pool_size.unwrap_or(20))
    .build(manager)
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  Ok(pool)
}

pub fn get_connection(pool: &Pool) -> io::Result<Connection> {
  let connection = pool
    .get()
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  Ok(connection)
}

pub fn select_postgis_verion(pool: &Pool) -> io::Result<String> {
  let mut connection = get_connection(pool)?;

  let version = connection
    .query_one(include_str!("scripts/get_postgis_version.sql"), &[])
    .map(|row| row.get::<_, String>("postgis_version"))
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  Ok(version)
}

pub fn check_postgis_version(
  required_postgis_version: &str,
  pool: &Pool,
) -> io::Result<bool> {
  let postgis_version = select_postgis_verion(&pool)?;

  let req = VersionReq::parse(required_postgis_version)
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  let version = Version::parse(postgis_version.as_str())
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  let matches = req.matches(&version);

  if !matches {
    error!(
      "Martin requires PostGIS {}, current version is {}",
      required_postgis_version, postgis_version
    );
  }

  Ok(matches)
}
