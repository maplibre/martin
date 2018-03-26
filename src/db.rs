use actix::prelude::{Actor, Handler, Message, SyncContext};
use r2d2_postgres::{PostgresConnectionManager, TlsMode};
use r2d2::{Pool, PooledConnection};
use std::error::Error;
use std::io;

use super::source::{get_sources, Source, Sources, Tile};

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

pub struct GetSources {}
impl Message for GetSources {
    type Result = Result<Sources, io::Error>;
}

impl Handler<GetSources> for DbExecutor {
    type Result = Result<Sources, io::Error>;

    fn handle(&mut self, _msg: GetSources, _: &mut Self::Context) -> Self::Result {
        let conn = self.0.get().unwrap();
        let sources = get_sources(conn)?;
        Ok(sources)
    }
}

#[derive(Debug)]
pub struct GetTile {
    pub z: u32,
    pub x: u32,
    pub y: u32,
    pub source: Source,
    pub condition: Option<String>,
}

impl Message for GetTile {
    type Result = Result<Tile, io::Error>;
}

impl Handler<GetTile> for DbExecutor {
    type Result = Result<Tile, io::Error>;

    fn handle(&mut self, msg: GetTile, _: &mut Self::Context) -> Self::Result {
        let conn = self.0.get().unwrap();

        let tile = msg.source
            .get_tile(conn, msg.z, msg.x, msg.y, msg.condition)?;

        Ok(tile)
    }
}
