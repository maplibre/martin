// use actix::prelude::{Actor, SyncContext};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use r2d2::{Config, Pool, PooledConnection};
use std::error::Error;

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresConnection = PooledConnection<PostgresConnectionManager>;

// pub struct DbExecutor(pub PostgresPool);

// impl Actor for DbExecutor {
//     type Context = SyncContext<Self>;
// }

pub fn setup_connection_pool(cn_str: &str, pool_size: u32) -> Result<PostgresPool, Box<Error>> {
    let config = Config::builder().pool_size(pool_size).build();
    let manager = PostgresConnectionManager::new(cn_str, TlsMode::None)?;
    let pool = Pool::new(config, manager)?;
    Ok(pool)
}
