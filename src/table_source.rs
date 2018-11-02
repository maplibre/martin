use mapbox_expressions_to_sql;
use std::collections::HashMap;
use std::error::Error;
use std::io;

use super::app::Query;
use super::db::PostgresConnection;
use super::source::{Source, Tile, XYZ};
use super::utils;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableSource {
  pub id: String,
  pub schema: String,
  pub table: String,
  pub id_column: Option<String>,
  pub geometry_column: String,
  pub srid: u32,
  pub extent: Option<u32>,
  pub buffer: Option<u32>,
  pub clip_geom: Option<bool>,
  pub geometry_type: Option<String>,
  pub properties: HashMap<String, String>,
}

pub type TableSources = HashMap<String, Box<TableSource>>;

impl Source for TableSource {
  fn get_id(&self) -> &str {
    self.id.as_str()
  }

  fn get_tile(
    &self,
    conn: &PostgresConnection,
    xyz: &XYZ,
    query: &Query,
  ) -> Result<Tile, io::Error> {
    let mercator_bounds = utils::tilebbox(xyz);

    let (geometry_column_mercator, original_bounds) = if self.srid == 3857 {
      (self.geometry_column.clone(), mercator_bounds.clone())
    } else {
      (
        format!("ST_Transform({0}, 3857)", self.geometry_column),
        format!("ST_Transform({0}, {1})", mercator_bounds, self.srid),
      )
    };

    let properties = if self.properties.is_empty() {
      "".to_string()
    } else {
      let properties = self
        .properties
        .keys()
        .map(|column| format!("\"{0}\"", column))
        .collect::<Vec<String>>()
        .join(",");

      format!(", {0}", properties)
    };

    let id_column = self
      .id_column
      .clone()
      .map_or("".to_string(), |id_column| format!(", '{}'", id_column));

    let condition = query
      .get("filter")
      .and_then(|filter| mapbox_expressions_to_sql::parse(filter).ok());

    let query = format!(
      include_str!("scripts/get_tile.sql"),
      id = self.id,
      id_column = id_column,
      geometry_column = self.geometry_column,
      geometry_column_mercator = geometry_column_mercator,
      original_bounds = original_bounds,
      mercator_bounds = mercator_bounds,
      extent = self.extent.unwrap_or(DEFAULT_EXTENT),
      buffer = self.buffer.unwrap_or(DEFAULT_BUFFER),
      clip_geom = self.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM),
      properties = properties,
      condition = condition.map_or("".to_string(), |condition| format!("AND {}", condition)),
    );

    let tile: Tile = conn
      .query(&query, &[])
      .map(|rows| rows.get(0).get("st_asmvt"))
      .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

    Ok(tile)
  }
}

static DEFAULT_EXTENT: u32 = 4096;
static DEFAULT_BUFFER: u32 = 64;
static DEFAULT_CLIP_GEOM: bool = true;

pub fn get_table_sources(conn: &PostgresConnection) -> Result<TableSources, io::Error> {
  let mut sources = HashMap::new();

  let rows = conn
    .query(include_str!("scripts/get_table_sources.sql"), &[])
    .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

  for row in &rows {
    let schema: String = row.get("f_table_schema");
    let table: String = row.get("f_table_name");
    let id = format!("{}.{}", schema, table);

    let geometry_column: String = row.get("f_geometry_column");
    let srid: i32 = row.get("srid");

    info!("{} table found", id);

    if srid == 0 {
      warn!("{} has SRID 0, skipping", id);
      continue;
    }

    let properties = utils::json_to_hashmap(&row.get("properties"));

    let source = TableSource {
      id: id.to_string(),
      schema,
      table,
      id_column: None,
      geometry_column,
      srid: srid as u32,
      extent: Some(DEFAULT_EXTENT),
      buffer: Some(DEFAULT_BUFFER),
      clip_geom: Some(DEFAULT_CLIP_GEOM),
      geometry_type: row.get("type"),
      properties,
    };

    sources.insert(id, Box::new(source));
  }

  Ok(sources)
}
