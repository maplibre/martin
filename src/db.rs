use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use std::error::Error;
use std::io;

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresConnection = PooledConnection<PostgresConnectionManager>;

pub fn setup_connection_pool(
  cn_str: &str,
  pool_size: Option<u32>,
) -> Result<PostgresPool, Box<Error>> {
  let manager = PostgresConnectionManager::new(cn_str, TlsMode::None)?;

  let pool = Pool::builder()
    .max_size(pool_size.unwrap_or(20))
    .build(manager)?;

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
