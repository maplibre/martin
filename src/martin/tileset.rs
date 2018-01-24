use iron::typemap::Key;
use serde_json;
use std::collections::HashMap;
use std::error::Error;

use super::db::PostgresConnection;

#[derive(Serialize, Debug)]
pub struct Tileset {
    schema: String,
    pub table: String,
    geometry_column: String,
    srid: i32,
    extent: i32,
    buffer: i32,
    clip_geom: bool,
    geometry_type: String,
    properties: HashMap<String, String>
}

impl Tileset {
    pub fn get_query(&self, condition: Option<String>) -> String {
        let keys: Vec<String> = self.properties.keys().map(|key| key.to_string()).collect();
        let columns = keys.join(",");

        let transformed_geometry = if self.srid == 3857 {
            self.geometry_column.clone()
        } else {
            format!("ST_Transform({0}, 3857)", self.geometry_column)
        };

        let query = format!(
            "SELECT ST_AsMVT(tile, '{1}', {4}, 'geom') FROM (\
                SELECT \
                    ST_AsMVTGeom({2}, TileBBox($1, $2, $3, 3857), {4}, {5}, {6}) AS geom, {3} \
                FROM {0}.{1} {7}\
            ) AS tile;",
            self.schema,
            self.table,
            transformed_geometry,
            columns,
            self.extent,
            self.buffer,
            self.clip_geom,
            condition.unwrap_or("".to_string())
        );

        query
    }
}

pub struct Tilesets;
impl Key for Tilesets { type Value = HashMap<String, Tileset>; }

fn value_to_hashmap(value: serde_json::Value) -> HashMap<String, String> {
    let mut hashmap = HashMap::new();

    let object = value.as_object().unwrap();
    for (key, value) in object {
        let string_value = value.as_str().unwrap();
        hashmap.insert(key.to_string(), string_value.to_string());
    };

    hashmap
}

pub fn get_tilesets(conn: PostgresConnection) -> Result<HashMap<String, Tileset>, Box<Error>> {
    let query = "
        SELECT
            f_table_schema, f_table_name, f_geometry_column, srid, type,
            json_object_agg(columns.column_name, columns.udt_name) as properties
        FROM geometry_columns
        LEFT JOIN information_schema.columns AS columns ON
            geometry_columns.f_table_schema = columns.table_schema AND
            geometry_columns.f_table_name = columns.table_name AND
            geometry_columns.f_geometry_column != columns.column_name
        GROUP BY f_table_schema, f_table_name, f_geometry_column, srid, type";

    let default_extent = 4096;
    let default_buffer = 0; // 256
    let default_clip_geom = true;

    let mut tilesets = HashMap::new();
    let rows = try!(conn.query(&query, &[]));

    for row in &rows {
        let schema: String = row.get("f_table_schema");
        let table: String = row.get("f_table_name");
        let id = format!("{}.{}", schema, table);

        let geometry_column: String = row.get("f_geometry_column");
        let srid: i32 = row.get("srid");

        let tileset = Tileset {
            schema: schema,
            table: table,
            geometry_column: geometry_column,
            srid: srid,
            extent: default_extent,
            buffer: default_buffer,
            clip_geom: default_clip_geom,
            geometry_type: row.get("type"),
            properties: value_to_hashmap(row.get("properties"))
        };

        tilesets.insert(id, tileset);
    }

    Ok(tilesets)
}

pub fn get_tile<'a>(conn: PostgresConnection, tileset: &Tileset, z: &i32, x: &i32, y: &i32, condition: Option<String>) -> Result<Vec<u8>, Box<Error>> {
    let rows = try!(conn.query(&tileset.get_query(condition), &[&z, &x, &y]));
    let tile = rows.get(0).get("st_asmvt");
    Ok(tile)
}