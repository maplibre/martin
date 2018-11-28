use actix::prelude::*;
use std::io;

use super::db::PostgresPool;
use super::function_source::{get_function_sources, FunctionSources};
use super::messages;
use super::source::Tile;
use super::table_source::{get_table_sources, TableSources};

pub struct DbExecutor(pub PostgresPool);

impl Actor for DbExecutor {
  type Context = SyncContext<Self>;
}

impl Handler<messages::GetTableSources> for DbExecutor {
  type Result = Result<TableSources, io::Error>;

  fn handle(&mut self, _msg: messages::GetTableSources, _: &mut Self::Context) -> Self::Result {
    let conn = self.0.get().unwrap();
    let table_sources = get_table_sources(&conn)?;
    Ok(table_sources)
  }
}

impl Handler<messages::GetFunctionSources> for DbExecutor {
  type Result = Result<FunctionSources, io::Error>;

  fn handle(&mut self, _msg: messages::GetFunctionSources, _: &mut Self::Context) -> Self::Result {
    let conn = self.0.get().unwrap();
    let function_sources = get_function_sources(&conn)?;
    Ok(function_sources)
  }
}

impl Handler<messages::GetTile> for DbExecutor {
  type Result = Result<Tile, io::Error>;

  fn handle(&mut self, msg: messages::GetTile, _: &mut Self::Context) -> Self::Result {
    let conn = self.0.get().unwrap();

    let tile = msg.source.get_tile(&conn, &msg.xyz, &msg.query)?;

    Ok(tile)
  }
}
