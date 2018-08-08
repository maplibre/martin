use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use std::error::Error;

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresConnection = PooledConnection<PostgresConnectionManager>;

pub fn setup_connection_pool(cn_str: &str, pool_size: u32) -> Result<PostgresPool, Box<Error>> {
    let manager = PostgresConnectionManager::new(cn_str, TlsMode::None)?;
    let pool = Pool::builder().max_size(pool_size).build(manager)?;
    Ok(pool)
}
