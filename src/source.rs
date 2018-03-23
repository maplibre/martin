use std::collections::HashMap;
use std::error::Error;

use super::utils;
use super::db::PostgresConnection;

#[derive(Clone, Debug, Serialize)]
pub struct Source {
    pub id: String,
    schema: String,
    table: String,
    pub geometry_column: String,
    pub srid: u32,
    pub extent: u32,
    pub buffer: u32,
    pub clip_geom: bool,
    geometry_type: String,
    pub properties: HashMap<String, String>,
}

pub type Sources = HashMap<String, Source>;

pub fn get_sources(conn: PostgresConnection) -> Result<HashMap<String, Source>, Box<Error>> {
    let query = "
        WITH columns AS (
            SELECT
                ns.nspname AS table_schema,
                class.relname AS table_name,
                attr.attname AS column_name,
                trim(leading '_' from tp.typname) AS type_name
            FROM pg_attribute attr
                JOIN pg_catalog.pg_class AS class ON class.oid = attr.attrelid
                JOIN pg_catalog.pg_namespace AS ns ON ns.oid = class.relnamespace
                JOIN pg_catalog.pg_type AS tp ON tp.oid = attr.atttypid
            WHERE NOT attr.attisdropped AND attr.attnum > 0)
        SELECT
            f_table_schema, f_table_name, f_geometry_column, srid, type,
            jsonb_object_agg(columns.column_name, columns.type_name) as properties
        FROM geometry_columns
        LEFT JOIN columns ON
            geometry_columns.f_table_schema = columns.table_schema AND
            geometry_columns.f_table_name = columns.table_name AND
            geometry_columns.f_geometry_column != columns.column_name
        GROUP BY f_table_schema, f_table_name, f_geometry_column, srid, type";

    let default_extent = 4096;
    let default_buffer = 0; // 256
    let default_clip_geom = true;

    let mut sources = HashMap::new();
    let rows = try!(conn.query(&query, &[]));

    for row in &rows {
        let schema: String = row.get("f_table_schema");
        let table: String = row.get("f_table_name");
        let id = format!("{}.{}", schema, table);

        let geometry_column: String = row.get("f_geometry_column");
        let srid: i32 = row.get("srid");

        if srid == 0 {
            warn!("{} has SRID 0", id);
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
