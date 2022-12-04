use crate::pg::config::{PgInfo, TableInfo};
use crate::pg::configurator::SqlTableInfoMapMapMap;
use crate::pg::connection::Pool;
use crate::pg::pg_source::PgSqlInfo;
use crate::pg::utils::{io_error, json_to_hashmap, polygon_to_bbox};
use log::warn;
use postgres_protocol::escape::{escape_identifier, escape_literal};
use std::collections::HashMap;
use std::io;

static DEFAULT_EXTENT: u32 = 4096;
static DEFAULT_BUFFER: u32 = 64;
static DEFAULT_CLIP_GEOM: bool = true;

#[derive(Clone, Debug)]
pub struct PgSqlTableInfo {
    pub info: TableInfo,
}

pub async fn get_table_sources(pool: &Pool) -> Result<SqlTableInfoMapMapMap, io::Error> {
    let conn = pool.get().await?;
    let rows = conn
        .query(include_str!("scripts/get_table_sources.sql"), &[])
        .await
        .map_err(|e| io_error!(e, "Can't get table sources"))?;

    let mut res = SqlTableInfoMapMapMap::new();
    for row in &rows {
        let info = TableInfo {
            schema: row.get("schema"),
            table: row.get("name"),
            geometry_column: row.get("geom"),
            srid: row.get("srid"), // casting i32 to u32?
            extent: Some(DEFAULT_EXTENT),
            buffer: Some(DEFAULT_BUFFER),
            clip_geom: Some(DEFAULT_CLIP_GEOM),
            geometry_type: row.get("type"),
            properties: json_to_hashmap(&row.get("properties")),
            unrecognized: HashMap::new(),
            ..TableInfo::default()
        };

        if let Some(v) = res
            .entry(info.schema.clone())
            .or_insert_with(HashMap::new)
            .entry(info.table.clone())
            .or_insert_with(HashMap::new)
            .insert(info.geometry_column.clone(), info)
        {
            warn!("Unexpected duplicate function {}", v.format_id());
        }
    }

    Ok(res)
}

pub async fn table_to_query(
    id: String,
    mut info: TableInfo,
    pool: Pool,
) -> Result<(String, PgSqlInfo, TableInfo), io::Error> {
    let bounds_query = format!(
        include_str!("scripts/get_bounds.sql"),
        schema = info.schema,
        table = info.table,
        srid = info.srid,
        geometry_column = info.geometry_column,
    );

    if info.bounds.is_none() {
        info.bounds = pool
            .get()
            .await?
            .query_one(bounds_query.as_str(), &[])
            .await
            .map(|row| row.get("bounds"))
            .ok()
            .flatten()
            .and_then(|v| polygon_to_bbox(&v));
    }

    let properties = if info.properties.is_empty() {
        String::new()
    } else {
        let properties = info
            .properties
            .keys()
            .map(|column| escape_identifier(column))
            .collect::<Vec<String>>()
            .join(",");
        format!(", {properties}")
    };

    let id_column = info
        .id_column
        .clone()
        .map_or(String::new(), |id_column| format!(", '{id_column}'"));

    // TODO: extend ST_TileEnvelope by the buffer size. In PostGIS 3.3, this will be possible with
    // the 5th margin parameter of the ST_TileEnvelope(z,x,y, srid, margin)

    let query = format!(
            r#"
    SELECT
      ST_AsMVT(tile, {table_id}, {extent}, 'geom' {id_column})
    FROM (
      SELECT
        ST_AsMVTGeom(
            ST_Transform(ST_CurveToLine({geometry_column}), 3857),
            ST_TileEnvelope($1::integer, $2::integer, $3::integer),
            {extent}, {buffer}, {clip_geom}
        ) AS geom
        {properties}
      FROM
        {schema}.{table}
      WHERE
        {geometry_column} && ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer), {srid})
    ) AS tile
    "#,
            table_id = escape_literal(info.format_id().as_str()),
            extent = info.extent.unwrap_or(DEFAULT_EXTENT),
            geometry_column = escape_identifier(&info.geometry_column),
            buffer = info.buffer.unwrap_or(DEFAULT_BUFFER),
            clip_geom = info.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM),
            schema = escape_identifier(&info.schema),
            table = escape_identifier(&info.table),
            srid = info.srid,
        ).trim().to_string();

    Ok((id, PgSqlInfo::new(query, false, info.format_id()), info))
}
