use actix::prelude::*;
use r2d2::{Pool, PooledConnection};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use std::error::Error;
use std::io;

use super::messages;
use super::source::{get_sources, Sources, Tile};

pub type PostgresPool = Pool<PostgresConnectionManager>;
pub type PostgresConnection = PooledConnection<PostgresConnectionManager>;

pub fn setup_connection_pool(cn_str: &str, pool_size: u32) -> Result<PostgresPool, Box<Error>> {
    let manager = PostgresConnectionManager::new(cn_str, TlsMode::None)?;
    let pool = Pool::builder().max_size(pool_size).build(manager)?;
    Ok(pool)
}

#[derive(Debug)]
pub struct DbExecutor(pub PostgresPool);

impl Actor for DbExecutor {
    type Context = SyncContext<Self>;
}

impl Handler<messages::GetSources> for DbExecutor {
    type Result = Result<Sources, io::Error>;

    fn handle(&mut self, _msg: messages::GetSources, _: &mut Self::Context) -> Self::Result {
        let conn = self.0.get().unwrap();
        let sources = get_sources(conn)?;
        Ok(sources)
    }
}

impl Handler<messages::GetTile> for DbExecutor {
    type Result = Result<Tile, io::Error>;

    fn handle(&mut self, msg: messages::GetTile, _: &mut Self::Context) -> Self::Result {
        let conn = self.0.get().unwrap();

        let tile = msg.source
            .get_tile(conn, msg.z, msg.x, msg.y, msg.condition)?;

        Ok(tile)
    }
}
