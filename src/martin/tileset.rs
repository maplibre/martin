use std::error::Error;
use std::collections::HashMap;
use iron::typemap::Key;

use super::db::PostgresConnection;

#[derive(Serialize, Debug)]
pub struct Tileset {
    schema: String,
    pub table: String,
    geometry_column: String,
    transformed_geometry: String,
    srid: i32,
    extent: i32,
    buffer: i32,
    clip_geom: bool,
    geometry_type: String
}

impl Tileset {
    pub fn get_query(&self, condition: Option<String>) -> String {
        let query = format!(
            "SELECT ST_AsMVT(q, '{1}', {3}, 'geom') FROM (\
                SELECT ST_AsMVTGeom(\
                    {2}, \
                    TileBBox($1, $2, $3, 3857), \
                    {3}, \
                    {4}, \
                    {5}\
                ) AS geom FROM {0}.{1} {6}\
            ) AS q;",
            self.schema,
            self.table,
            self.transformed_geometry,
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

pub fn get_tilesets(conn: PostgresConnection) -> Result<HashMap<String, Tileset>, Box<Error>> {
    let query = "
        select
            f_table_schema, f_table_name, f_geometry_column, srid, type
        from geometry_columns
    ";

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

        let transformed_geometry = if srid == 3857 {
            geometry_column.clone()
        } else {
            format!("ST_Transform({0}, 3857)", geometry_column)
        };

        let tileset = Tileset {
            schema: schema,
            table: table,
            geometry_column: geometry_column,
            transformed_geometry: transformed_geometry,
            srid: srid,
            extent: default_extent,
            buffer: default_buffer,
            clip_geom: default_clip_geom,
            geometry_type: row.get("type")
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