use actix::{Actor, Handler, SyncContext};
use std::io;

use crate::db::PostgresPool;
use crate::function_source::{get_function_sources, FunctionSources};
use crate::messages;
use crate::source::Tile;
use crate::table_source::{get_table_sources, TableSources};

pub struct DBActor(pub PostgresPool);

impl Actor for DBActor {
  type Context = SyncContext<Self>;
}

impl Handler<messages::GetTableSources> for DBActor {
  type Result = Result<TableSources, io::Error>;

  fn handle(&mut self, _msg: messages::GetTableSources, _: &mut Self::Context) -> Self::Result {
    let conn = self.0.get().unwrap();
    let table_sources = get_table_sources(&conn)?;
    Ok(table_sources)
  }
}

impl Handler<messages::GetFunctionSources> for DBActor {
  type Result = Result<FunctionSources, io::Error>;

  fn handle(&mut self, _msg: messages::GetFunctionSources, _: &mut Self::Context) -> Self::Result {
    let conn = self.0.get().unwrap();
    let function_sources = get_function_sources(&conn)?;
    Ok(function_sources)
  }
}

impl Handler<messages::GetTile> for DBActor {
  type Result = Result<Tile, io::Error>;

  fn handle(&mut self, msg: messages::GetTile, _: &mut Self::Context) -> Self::Result {
    let conn = self.0.get().unwrap();

    let tile = msg.source.get_tile(&conn, &msg.xyz, &msg.query)?;

    Ok(tile)
  }
}
