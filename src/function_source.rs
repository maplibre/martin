use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use tilejson::{TileJSON, TileJSONBuilder};

use crate::db::PostgresConnection;
use crate::source::{Query, Source, Tile, XYZ};
use crate::utils::query_to_json_string;

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

  fn get_tilejson(&self) -> Result<TileJSON, io::Error> {
    let mut tilejson_builder = TileJSONBuilder::new();

    tilejson_builder.scheme("xyz");
    tilejson_builder.name(&self.id);
    tilejson_builder.tiles(vec![]);

    Ok(tilejson_builder.finalize())
  }

  fn get_tile(
    &self,
    conn: &PostgresConnection,
    xyz: &XYZ,
    query: &Option<Query>,
  ) -> Result<Tile, io::Error> {
    let empty_query = HashMap::new();
    let query = query.as_ref().unwrap_or(&empty_query);

    let query_json_string =
      query_to_json_string(&query).map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;

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
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.to_string()))?;

  for row in &rows {
    let schema: String = row.get("specific_schema");
    let function: String = row.get("routine_name");
    let id = format!("{}.{}", schema, function);

    info!("Found {} function source", id);

    let source = FunctionSource {
      id: id.clone(),
      schema,
      function,
    };

    sources.insert(id, Box::new(source));
  }

  Ok(sources)
}
