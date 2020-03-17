use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use semver::Version;
use semver::VersionReq;
use std::io;

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresConnection = PooledConnection<PostgresConnectionManager>;

pub fn setup_connection_pool(cn_str: &str, pool_size: Option<u32>) -> io::Result<PostgresPool> {
  let manager = PostgresConnectionManager::new(cn_str, TlsMode::None)?;

  let pool = Pool::builder()
    .max_size(pool_size.unwrap_or(20))
    .build(manager)
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  Ok(pool)
}

pub fn select_postgis_verion(pool: &PostgresPool) -> io::Result<String> {
  let conn = pool
    .get()
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  let version: String = conn
    .query(r#"select (regexp_matches(postgis_lib_version(), '^(\d+\.\d+\.\d+)', 'g'))[1] as postgis_lib_version"#, &[])
    .map(|rows| rows.get(0).get("postgis_lib_version"))?;

  Ok(version)
}

pub fn check_postgis_version(
  required_postgis_version: &str,
  pool: &PostgresPool,
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
