use std::collections::HashMap;
use std::io;

use super::db::PostgresConnection;
use super::martin::Query;
use super::source::{Source, Tile, XYZ};
use super::utils::query_to_json_string;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionSource {
  id: String,
  schema: String,
  function: String,
}

impl Source for FunctionSource {
  fn get_id(&self) -> &str {
    self.id.as_str()
  }

  fn get_tile(&self, conn: PostgresConnection, xyz: XYZ, query: Query) -> Result<Tile, io::Error> {
    let query_json_string =
      query_to_json_string(query).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    let query = format!(
      include_str!("scripts/call_rpc.sql"),
      schema = self.schema,
      function = self.function,
      z = xyz.z,
      x = xyz.x,
      y = xyz.y,
      query = query_json_string
    );

    let tile: Tile = conn
      .query(&query, &[])
      .map(|rows| rows.get(0).get(self.function.as_str()))
      .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    Ok(tile)
  }
}

pub type FunctionSources = HashMap<String, Box<FunctionSource>>;
