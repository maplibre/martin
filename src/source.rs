use std::collections::HashMap;
use std::error::Error;
use std::io;

use super::db::PostgresConnection;
use super::utils;

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub id: String,
    pub schema: String,
    pub table: String,
    pub geometry_column: String,
    pub srid: u32,
    pub extent: u32,
    pub buffer: u32,
    pub clip_geom: bool,
    pub geometry_type: String,
    pub properties: HashMap<String, String>,
}

pub type Tile = Vec<u8>;

impl Source {
    pub fn get_tile(
        &self,
        conn: PostgresConnection,
        z: u32,
        x: u32,
        y: u32,
        condition: Option<String>,
    ) -> Result<Tile, io::Error> {
        let mercator_bounds = utils::tilebbox(z, x, y);

        let (geometry_column_mercator, original_bounds) = if self.srid == 3857 {
            (self.geometry_column.clone(), mercator_bounds.clone())
        } else {
            (
                format!("ST_Transform({0}, 3857)", self.geometry_column),
                format!("ST_Transform({0}, {1})", mercator_bounds, self.srid),
            )
        };

        let columns: Vec<String> = self.properties
            .keys()
            .map(|column| format!("\"{0}\"", column))
            .collect();

        let properties = columns.join(",");

        let query = format!(
            include_str!("scripts/get_tile.sql"),
            id = self.id,
            geometry_column = self.geometry_column,
            geometry_column_mercator = geometry_column_mercator,
            original_bounds = original_bounds,
            mercator_bounds = mercator_bounds,
            extent = self.extent,
            buffer = self.buffer,
            clip_geom = self.clip_geom,
            properties = properties,
            condition = condition.map_or("".to_string(), |condition| format!("AND {}", condition)),
        );

        let tile: Tile = conn.query(&query, &[])
            .map(|rows| rows.get(0).get("st_asmvt"))
            .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

        Ok(tile)
    }
}

pub type Sources = HashMap<String, Source>;

pub fn get_sources(conn: PostgresConnection) -> Result<Sources, io::Error> {
    let default_extent = 4096;
    let default_buffer = 64;
    let default_clip_geom = true;

    let mut sources = HashMap::new();
    let rows = conn.query(include_str!("scripts/get_sources.sql"), &[])
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err.description()))?;

    for row in &rows {
        let schema: String = row.get("f_table_schema");
        let table: String = row.get("f_table_name");
        let id = format!("{}.{}", schema, table);

        let geometry_column: String = row.get("f_geometry_column");
        let srid: i32 = row.get("srid");

        if srid == 0 {
            warn!("{} has SRID 0, skipping", id);
            continue;
        }

        let source = Source {
            id: id.to_string(),
            schema: schema,
            table: table,
            geometry_column: geometry_column,
            srid: srid as u32,
            extent: default_extent,
            buffer: default_buffer,
            clip_geom: default_clip_geom,
            geometry_type: row.get("type"),
            properties: utils::json_to_hashmap(row.get("properties")),
        };

        sources.insert(id, source);
    }

    Ok(sources)
}
