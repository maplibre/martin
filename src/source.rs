// use std::collections::HashMap;
use std::fmt::Debug;
use std::io;

use super::db::PostgresConnection;
use super::martin::Query;

pub type Tile = Vec<u8>;

pub struct XYZ {
  pub z: u32,
  pub x: u32,
  pub y: u32,
}

pub trait Source: Debug {
  fn get_id(&self) -> &str;
  fn get_tile(&self, conn: PostgresConnection, xyz: XYZ, query: Query) -> Result<Tile, io::Error>;
}

// pub type Sources = HashMap<String, Box<dyn Source>>;
