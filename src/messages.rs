use actix::prelude::*;
use std::io;

use super::app::Query;
use super::source::{Source, Tile, XYZ};

pub struct GetTile {
  pub xyz: XYZ,
  pub query: Query,
  pub source: Box<dyn Source + Send>,
}

impl Message for GetTile {
  type Result = Result<Tile, io::Error>;
}
