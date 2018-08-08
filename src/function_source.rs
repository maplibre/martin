use std::collections::HashMap;
use std::io;

use super::db::PostgresConnection;
use super::martin::Query;
use super::source::{Source, Tile, XYZ};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionSource {
  id: String,
}

impl Source for FunctionSource {
  fn get_id(self) -> String {
    self.id
  }

  fn get_tile(
    &self,
    _conn: PostgresConnection,
    _xyz: XYZ,
    _query: Query,
  ) -> Result<Tile, io::Error> {
    Ok(Vec::new())
  }
}

pub type FunctionSources = HashMap<String, Box<FunctionSource>>;
