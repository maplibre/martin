use std::collections::HashMap;
use std::error::Error;
use std::io;

use super::app::Query;
use super::db::PostgresConnection;
use super::source::{Source, Tile, XYZ};
use super::utils::query_to_json_string;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FunctionSource {
  pub id: String,
  pub schema: String,
  pub function: String,
}

pub type FunctionSources = HashMap<String, Box<FunctionSource>>;

impl Source for FunctionSource {
  fn get_id(&self) -> &str {
    self.id.as_str()
  }

  fn get_tile(
    &self,
    conn: &PostgresConnection,
    xyz: &XYZ,
    query: &Query,
  ) -> Result<Tile, io::Error> {
    let query_json_string =
      query_to_json_string(query).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    let query = format!(
      include_str!("scripts/call_rpc.sql"),
      schema = self.schema,
      function = self.function,
      z = xyz.z,
      x = xyz.x,
      y = xyz.y,
      query_params = query_json_string
    );

    let tile: Tile = conn
      .query(&query, &[])
      .map(|rows| rows.get(0).get(self.function.as_str()))
      .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

    Ok(tile)
  }
}

pub fn get_function_sources(conn: &PostgresConnection) -> Result<FunctionSources, io::Error> {
  let mut sources = HashMap::new();

  let rows = conn
    .query(include_str!("scripts/get_function_sources.sql"), &[])
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

  for row in &rows {
    let schema: String = row.get("specific_schema");
    let function: String = row.get("routine_name");
    let id = format!("{}.{}", schema, function);

    info!("{} function found", id);

    let source = FunctionSource {
      id: id.clone(),
      schema,
      function,
    };

    sources.insert(id, Box::new(source));
  }

  Ok(sources)
}
