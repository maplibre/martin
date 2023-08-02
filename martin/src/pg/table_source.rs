use std::collections::HashMap;

use log::{info, warn};
use postgis::ewkb;
use postgres_protocol::escape::{escape_identifier, escape_literal};
use tilejson::Bounds;

use crate::pg::config::PgInfo;
use crate::pg::config_table::TableInfo;
use crate::pg::configurator::SqlTableInfoMapMapMap;
use crate::pg::pg_source::PgSqlInfo;
use crate::pg::pool::PgPool;
use crate::pg::utils::PgError::PostgresError;
use crate::pg::utils::{json_to_hashmap, polygon_to_bbox, Result};
use crate::utils::normalize_key;

static DEFAULT_EXTENT: u32 = 4096;
static DEFAULT_BUFFER: u32 = 64;
static DEFAULT_CLIP_GEOM: bool = true;

pub async fn get_table_sources(pool: &PgPool) -> Result<SqlTableInfoMapMapMap> {
    let conn = pool.get().await?;
    let rows = conn
        .query(include_str!("scripts/get_table_sources.sql"), &[])
        .await
        .map_err(|e| PostgresError(e, "querying available tables"))?;

    let mut res = SqlTableInfoMapMapMap::new();
    for row in &rows {
        let info = TableInfo {
            layer_id: None,
            schema: row.get("schema"),
            table: row.get("name"),
            geometry_column: row.get("geom"),
            geometry_index: row.get("geom_idx"),
            is_view: row.get("is_view"),
            id_column: None,
            minzoom: None,
            maxzoom: None,
            srid: row.get("srid"), // casting i32 to u32?
            extent: Some(DEFAULT_EXTENT),
            buffer: Some(DEFAULT_BUFFER),
            clip_geom: Some(DEFAULT_CLIP_GEOM),
            geometry_type: row.get("type"),
            properties: Some(json_to_hashmap(&row.get("properties"))),
            prop_mapping: HashMap::new(),
            unrecognized: HashMap::new(),
            bounds: None,
        };

        // Warn for missing geometry indices. Ignore views since those can't have indices
        // and will generally refer to table columns.
        if let (Some(false), Some(false)) = (info.geometry_index, info.is_view) {
            warn!(
                "Table {}.{} has no spatial index on column {}",
                info.schema, info.table, info.geometry_column
            );
        }

        if let Some(v) = res
            .entry(info.schema.clone())
            .or_insert_with(HashMap::new)
            .entry(info.table.clone())
            .or_insert_with(HashMap::new)
            .insert(info.geometry_column.clone(), info)
        {
            warn!("Unexpected duplicate table {}", v.format_id());
        }
    }

    Ok(res)
}

fn escape_with_alias(mapping: &HashMap<String, String>, field: &str) -> String {
    let column = mapping.get(field).map_or(field, |v| v.as_str());
    if field == column {
        format!(", {}", escape_identifier(column))
    } else {
        format!(
            ", {} AS {}",
            escape_identifier(column),
            escape_identifier(field),
        )
    }
}

pub async fn table_to_query(
    id: String,
    mut info: TableInfo,
    pool: PgPool,
    disable_bounds: bool,
    max_feature_count: Option<usize>,
) -> Result<(String, PgSqlInfo, TableInfo)> {
    let schema = escape_identifier(&info.schema);
    let table = escape_identifier(&info.table);
    let geometry_column = escape_identifier(&info.geometry_column);
    let srid = info.srid;

    if info.bounds.is_none() && !disable_bounds {
        info.bounds = calc_bounds(&pool, &schema, &table, &geometry_column, srid).await?;
    }

    let properties = if let Some(props) = &info.properties {
        props
            .keys()
            .map(|column| escape_with_alias(&info.prop_mapping, column))
            .collect::<String>()
    } else {
        String::new()
    };

    let (id_name, id_field) = if let Some(id_column) = &info.id_column {
        (
            format!(", {}", escape_literal(id_column)),
            escape_with_alias(&info.prop_mapping, id_column),
        )
    } else {
        (String::new(), String::new())
    };

    let extent = info.extent.unwrap_or(DEFAULT_EXTENT);
    let buffer = info.buffer.unwrap_or(DEFAULT_BUFFER);

    let bbox_search = if buffer == 0 {
        "ST_TileEnvelope($1::integer, $2::integer, $3::integer)".to_string()
    } else if pool.supports_tile_margin() {
        let margin = f64::from(buffer) / f64::from(extent);
        format!("ST_TileEnvelope($1::integer, $2::integer, $3::integer, margin => {margin})")
    } else {
        // TODO: we should use ST_Expand here, but it may require a bit more math work,
        //       so might not be worth it as it is only used for PostGIS < v3.1.
        //       v3.1 has been out for 2+ years (december 2020)
        // let earth_circumference = 40075016.6855785;
        // let val = earth_circumference * buffer as f64 / extent as f64;
        // format!("ST_Expand(ST_TileEnvelope($1::integer, $2::integer, $3::integer), {val}/2^$1::integer)")
        "ST_TileEnvelope($1::integer, $2::integer, $3::integer)".to_string()
    };

    let limit_clause = max_feature_count.map_or(String::new(), |v| format!("LIMIT {v}"));
    let layer_id = escape_literal(info.layer_id.as_ref().unwrap_or(&id));
    let clip_geom = info.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM);
    let query = format!(
        r#"
SELECT
  ST_AsMVT(tile, {layer_id}, {extent}, 'geom'{id_name})
FROM (
  SELECT
    ST_AsMVTGeom(
        ST_Transform(ST_CurveToLine({geometry_column}), 3857),
        ST_TileEnvelope($1::integer, $2::integer, $3::integer),
        {extent}, {buffer}, {clip_geom}
    ) AS geom
    {id_field}{properties}
  FROM
    {schema}.{table}
  WHERE
    {geometry_column} && ST_Transform({bbox_search}, {srid})
  {limit_clause}
) AS tile;
"#
    )
    .trim()
    .to_string();

    Ok((id, PgSqlInfo::new(query, false, info.format_id()), info))
}

async fn calc_bounds(
    pool: &PgPool,
    schema: &str,
    table: &str,
    geometry_column: &str,
    srid: i32,
) -> Result<Option<Bounds>> {
    Ok(pool.get()
        .await?
        .query_one(&format!(
            r#"
WITH real_bounds AS (SELECT ST_SetSRID(ST_Extent({geometry_column}), {srid}) AS rb FROM {schema}.{table})
SELECT ST_Transform(
            CASE
                WHEN (SELECT ST_GeometryType(rb) FROM real_bounds LIMIT 1) = 'ST_Point'
                THEN ST_SetSRID(ST_Extent(ST_Expand({geometry_column}, 1)), {srid})
                ELSE (SELECT * FROM real_bounds)
            END,
            4326
        ) AS bounds
FROM {schema}.{table};
                "#), &[])
        .await
        .map_err(|e| PostgresError(e, "querying table bounds"))?
        .get::<_, Option<ewkb::Polygon>>("bounds")
        .and_then(|p| polygon_to_bbox(&p)))
}

#[must_use]
pub fn merge_table_info(
    default_srid: Option<i32>,
    new_id: &String,
    cfg_inf: &TableInfo,
    src_inf: &TableInfo,
    auto_id_columns: &Vec<String>,
) -> Option<TableInfo> {
    // Assume cfg_inf and src_inf have the same schema/table/geometry_column
    let table_id = src_inf.format_id();
    let mut inf = TableInfo {
        // These values must match the database exactly
        schema: src_inf.schema.clone(),
        table: src_inf.table.clone(),
        geometry_column: src_inf.geometry_column.clone(),
        geometry_index: src_inf.geometry_index,
        is_view: src_inf.is_view,
        srid: calc_srid(&table_id, new_id, src_inf.srid, cfg_inf.srid, default_srid)?,
        prop_mapping: HashMap::new(),
        ..cfg_inf.clone()
    };

    match (&src_inf.geometry_type, &cfg_inf.geometry_type) {
        (Some(src), Some(cfg)) if src != cfg => {
            warn!(r#"Table {table_id} has geometry type={src}, but source {new_id} has {cfg}"#);
        }
        _ => {}
    }

    let empty = HashMap::new();
    let props = src_inf.properties.as_ref().unwrap_or(&empty);

    if let Some(id_column) = &cfg_inf.id_column {
        let prop = normalize_key(props, id_column.as_str(), "id_column", new_id)?;
        inf.prop_mapping.insert(id_column.clone(), prop);
    } else if !auto_id_columns.is_empty() {
        for s in auto_id_columns.iter() {
            if let Some(val) = props.get(s) {
                if val == "int4" {
                    inf.id_column = Some(s.to_string());
                    break;
                }
                warn!("Cannot use property {s} of type {val} as id column for {}.{}. Id column should be of type integer.", inf.schema, inf.table);
            }
        }
        if inf.id_column.is_none() {
            info!("No id column found for table {}.{}. Searched for columns with names {} and datatype integer.", inf.schema, inf.table, auto_id_columns.join(", "));
        }
    }

    if let Some(p) = &cfg_inf.properties {
        for key in p.keys() {
            let prop = normalize_key(props, key.as_str(), "property", new_id)?;
            inf.prop_mapping.insert(key.clone(), prop);
        }
    }

    Some(inf)
}

#[must_use]
pub fn calc_srid(
    table_id: &str,
    new_id: &str,
    src_srid: i32,
    cfg_srid: i32,
    default_srid: Option<i32>,
) -> Option<i32> {
    match (src_srid, cfg_srid, default_srid) {
        (0, 0, Some(default_srid)) => {
            info!("Table {table_id} has SRID=0, using provided default SRID={default_srid}");
            Some(default_srid)
        }
        (0, 0, None) => {
            // TODO: cleanup
            // println!("{:#?}", std::backtrace::Backtrace::force_capture());
            let info = "To use this table source, set default or specify this table SRID in the config file, or set the default SRID with  --default-srid=...";
            warn!("Table {table_id} has SRID=0, skipping. {info}");
            None
        }
        (0, cfg, _) => Some(cfg), // Use the configured SRID
        (src, 0, _) => Some(src), // Use the source SRID
        (src, cfg, _) if src != cfg => {
            warn!("Table {table_id} has SRID={src}, but source {new_id} has SRID={cfg}");
            None
        }
        (_, cfg, _) => Some(cfg),
    }
}
