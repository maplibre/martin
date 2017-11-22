use std::error::Error;
use iron::typemap::Key;
use r2d2::{Config, Pool, PooledConnection};
use r2d2_postgres::{TlsMode, PostgresConnectionManager};

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresPooledConnection = PooledConnection<PostgresConnectionManager>;

pub struct DB;
impl Key for DB { type Value = PostgresPool; }

pub fn setup_connection_pool(cn_str: &str, pool_size: u32) -> Result<PostgresPool, Box<Error>> {
    let config = Config::builder().pool_size(pool_size).build();
    let manager = try!(PostgresConnectionManager::new(cn_str, TlsMode::None));
    let pool = try!(Pool::new(config, manager));
    Ok(pool)
}
