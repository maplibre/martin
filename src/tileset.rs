use std::error::Error;
use std::collections::HashMap;
use iron::typemap::Key;

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
    query: String
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

        let query = format!(
            "SELECT ST_AsMVT(q, '{1}', {4}, '{2}') FROM (\
                SELECT ST_AsMVTGeom(\
                    {3}, \
                    TileBBox($1, $2, $3, 3857), \
                    {4}, \
                    {5}, \
                    {6}\
                ) AS geom FROM {0}.{1}\
            ) AS q;",
            schema,
            table,
            geometry_column,
            transformed_geometry,
            default_extent,
            default_buffer,
            default_clip_geom
        );

        let tileset = Tileset {
            schema: schema,
            table: table,
            geometry_column: geometry_column,
            srid: srid,
            extent: default_extent,
            buffer: default_buffer,
            clip_geom: default_clip_geom,
            geometry_type: row.get("type"),
            query: query
        };

        tilesets.insert(id, tileset);
    }

    Ok(tilesets)
}

pub fn get_tile(conn: PostgresConnection, tileset: &Tileset, z: &i32, x: &i32, y: &i32) -> Result<Vec<u8>, Box<Error>> {
    let rows = try!(conn.query(&tileset.query, &[&z, &x, &y]));
    let tile = rows.get(0).get("st_asmvt");
    Ok(tile)
}