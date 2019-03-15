use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use semver::Version;
use semver::VersionReq;
use std::error::Error;
use std::io;

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresConnection = PooledConnection<PostgresConnectionManager>;

pub fn setup_connection_pool(cn_str: &str, pool_size: Option<u32>) -> io::Result<PostgresPool> {
  let manager = PostgresConnectionManager::new(cn_str, TlsMode::None)?;

  let pool = Pool::builder()
    .max_size(pool_size.unwrap_or(20))
    .build(manager)
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

  Ok(pool)
}

pub fn select_postgis_verion(pool: &PostgresPool) -> io::Result<String> {
  let conn = pool
    .get()
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

  let version: String = conn
    .query("select postgis_lib_version()", &[])
    .map(|rows| rows.get(0).get("postgis_lib_version"))?;

  Ok(version)
}

pub fn check_postgis_version(required_postgis_version: &str, pool: &PostgresPool) {
  match select_postgis_verion(&pool) {
    Ok(postgis_version) => {
      let req = VersionReq::parse(required_postgis_version).unwrap();
      let version = Version::parse(postgis_version.as_str()).unwrap();
      if !req.matches(&version) {
        error!(
          "Martin requires PostGIS {}, current version is {}",
          required_postgis_version, postgis_version
        );
        std::process::exit(-1);
      }
    }
    Err(error) => {
      error!("Can't get PostGIS version: {}", error);
      error!("Martin requires PostGIS {}", required_postgis_version);
      std::process::exit(-1);
    }
  };
}
