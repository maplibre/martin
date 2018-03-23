use actix::prelude::{Actor, Handler, Message, SyncContext};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use r2d2::{Config, Pool, PooledConnection};
use std::error::Error;
use std::io;

use super::source::Source;

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresConnection = PooledConnection<PostgresConnectionManager>;

pub fn setup_connection_pool(cn_str: &str, pool_size: u32) -> Result<PostgresPool, Box<Error>> {
    let config = Config::builder().pool_size(pool_size).build();
    let manager = PostgresConnectionManager::new(cn_str, TlsMode::None)?;
    let pool = Pool::new(config, manager)?;
    Ok(pool)
}

#[derive(Debug)]
pub struct DbExecutor(pub PostgresPool);

impl Actor for DbExecutor {
    type Context = SyncContext<Self>;
}

#[derive(Debug)]
pub struct GetTile {
    pub source: Source,
    pub z: u32,
    pub x: u32,
    pub y: u32,
}

impl Message for GetTile {
    type Result = Result<Vec<u8>, io::Error>;
}

impl Handler<GetTile> for DbExecutor {
    type Result = Result<Vec<u8>, io::Error>;

    fn handle(&mut self, msg: GetTile, _: &mut Self::Context) -> Self::Result {
        let conn = self.0.get().unwrap();
        let query = msg.source.get_query(msg.z, msg.x, msg.y, None);

        let tile: Vec<u8> = conn.query(&query, &[])
            .map(|rows| rows.get(0).get("st_asmvt"))
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "db error"))?;

        Ok(tile)
    }
}
