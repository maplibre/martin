use actix::prelude::*;
use std::io;

use super::db::PostgresPool;
use super::messages;
use super::source::Tile;

pub struct DbExecutor(pub PostgresPool);

impl Actor for DbExecutor {
  type Context = SyncContext<Self>;
}

impl Handler<messages::GetTile> for DbExecutor {
  type Result = Result<Tile, io::Error>;

  fn handle(&mut self, msg: messages::GetTile, _: &mut Self::Context) -> Self::Result {
    let conn = self.0.get().unwrap();

    let tile = msg.source.get_tile(conn, msg.xyz, msg.query)?;

    Ok(tile)
  }
}
