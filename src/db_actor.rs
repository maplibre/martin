use std::io;

use actix::{Actor, Handler, SyncContext};

use crate::db::{get_connection, Pool};
use crate::function_source::{get_function_sources, FunctionSources};
use crate::messages;
use crate::source::Tile;
use crate::table_source::{get_table_sources, TableSources};

pub struct DbActor(pub Pool);

impl Actor for DbActor {
    type Context = SyncContext<Self>;
}

impl Handler<messages::GetTableSources> for DbActor {
    type Result = Result<TableSources, io::Error>;

    fn handle(&mut self, msg: messages::GetTableSources, _: &mut Self::Context) -> Self::Result {
        let mut connection = get_connection(&self.0)?;
        let table_sources = get_table_sources(&mut connection, &msg.default_srid)?;
        Ok(table_sources)
    }
}

impl Handler<messages::GetFunctionSources> for DbActor {
    type Result = Result<FunctionSources, io::Error>;

    fn handle(
        &mut self,
        _msg: messages::GetFunctionSources,
        _: &mut Self::Context,
    ) -> Self::Result {
        let mut connection = get_connection(&self.0)?;
        let function_sources = get_function_sources(&mut connection)?;
        Ok(function_sources)
    }
}

impl Handler<messages::GetTile> for DbActor {
    type Result = Result<Tile, io::Error>;

    fn handle(&mut self, msg: messages::GetTile, _: &mut Self::Context) -> Self::Result {
        let mut connection = get_connection(&self.0)?;
        let tile = msg.source.get_tile(&mut connection, &msg.xyz, &msg.query)?;

        Ok(tile)
    }
}
