use actix::prelude::*;
use std::io;

use super::source::{Source, Sources, Tile};

pub struct GetSources {}

impl Message for GetSources {
  type Result = Result<Sources, io::Error>;
}

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
