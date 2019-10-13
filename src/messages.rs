use actix::{Addr, Message};
use std::io;

use super::function_source::FunctionSources;
use super::source::{Query, Source, Tile, XYZ};
use super::table_source::TableSources;
use super::worker_actor::WorkerActor;

pub struct Connect {
  pub addr: Addr<WorkerActor>,
}

impl Message for Connect {
  type Result = Addr<WorkerActor>;
}

pub struct GetTile {
  pub xyz: XYZ,
  pub query: Query,
  pub source: Box<dyn Source + Send>,
}

impl Message for GetTile {
  type Result = Result<Tile, io::Error>;
}

pub struct GetTableSources {}
impl Message for GetTableSources {
  type Result = Result<TableSources, io::Error>;
}

pub struct GetFunctionSources {}
impl Message for GetFunctionSources {
  type Result = Result<FunctionSources, io::Error>;
}

pub struct RefreshTableSources {
  pub table_sources: Option<TableSources>,
}

impl Message for RefreshTableSources {
  type Result = ();
}

pub struct RefreshFunctionSources {
  pub function_sources: Option<FunctionSources>,
}

impl Message for RefreshFunctionSources {
  type Result = ();
}
