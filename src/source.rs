use std::fmt::Debug;
use std::io;

use super::app::Query;
use super::db::PostgresConnection;

pub type Tile = Vec<u8>;

#[derive(Copy, Clone)]
pub struct XYZ {
  pub z: u32,
  pub x: u32,
  pub y: u32,
}

pub trait Source: Debug {
  fn get_id(&self) -> &str;
  fn get_tile(
    &self,
    conn: &PostgresConnection,
    xyz: &XYZ,
    query: &Query,
  ) -> Result<Tile, io::Error>;
}

// pub type Sources = HashMap<String, Box<dyn Source>>;
