use std::collections::HashMap;

use futures::pin_mut;
use log::{debug, warn};
use postgis::ewkb;
use postgres_protocol::escape::{escape_identifier, escape_literal};
use serde_json::Value;
use tilejson::Bounds;
use tokio::time::timeout;

use crate::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::pg::builder::SqlTableInfoMapMapMap;
use crate::pg::config::PgInfo;
use crate::pg::config_table::TableInfo;
use crate::pg::pg_source::PgSqlInfo;
use crate::pg::pool::PgPool;
use crate::pg::utils::{json_to_hashmap, polygon_to_bbox};
use crate::pg::PgError::PostgresError;
use crate::pg::PgResult;

static DEFAULT_EXTENT: u32 = 4096;
static DEFAULT_BUFFER: u32 = 64;
static DEFAULT_CLIP_GEOM: bool = true;

/// Examine a database to get a list of all tables that have geometry columns.
pub async fn query_available_tables(pool: &PgPool) -> PgResult<SqlTableInfoMapMapMap> {
    let rows = pool
        .get()
        .await?
        .query(include_str!("scripts/query_available_tables.sql"), &[])
        .await
        .map_err(|e| PostgresError(e, "querying available tables"))?;

    let mut res = SqlTableInfoMapMapMap::new();
    for row in &rows {
        let schema: String = row.get("schema");
        let table: String = row.get("name");
        let tilejson = if let Some(text) = row.get("description") {
            match serde_json::from_str::<Value>(text) {
                Ok(v) => Some(v),
                Err(e) => {
                    warn!("Unable to deserialize SQL comment on {schema}.{table} as tilejson, the automatically generated tilejson would be used: {e}");
                    None
                }
            }
        } else {
            debug!("Unable to find a  SQL comment on {schema}.{table}, the tilejson would be generated automatically");
            None
        };

        let info = TableInfo {
            schema,
            table,
            geometry_column: row.get("geom"),
            geometry_index: row.get("geom_idx"),
            is_view: row.get("is_view"),
            srid: row.get("srid"), // casting i32 to u32?
            geometry_type: row.get("type"),
            properties: Some(json_to_hashmap(&row.get("properties"))),
            tilejson,
            ..Default::default()
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
            .or_default()
            .entry(info.table.clone())
            .or_default()
            .insert(info.geometry_column.clone(), info)
        {
            warn!("Unexpected duplicate table {}", v.format_id());
        }
    }

    Ok(res)
}

/// Generate an SQL snippet to escape a column name, and optionally alias it.
/// Assumes to not be the first column in a SELECT statement.
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

#[allow(clippy::too_many_lines)]
/// Generate a query to fetch tiles from a table.
/// The function is async because it may need to query the database for the table bounds (could be very slow).
pub async fn table_to_query(
    id: String,
    mut info: TableInfo,
    pool: PgPool,
    bounds_type: BoundsCalcType,
    max_feature_count: Option<usize>,
) -> PgResult<(String, PgSqlInfo, TableInfo)> {
    let srid = info.srid;

    if info.bounds.is_none() {
        match bounds_type {
            BoundsCalcType::Skip => {}
            BoundsCalcType::Calc => {
                debug!("Computing {} table bounds for {id}", info.format_id());
                info.bounds = calc_bounds(&pool, &info, srid, false).await?;
            }
            BoundsCalcType::Quick => {
                debug!(
                    "Computing {} table bounds with {}s timeout for {id}",
                    info.format_id(),
                    DEFAULT_BOUNDS_TIMEOUT.as_secs()
                );
                let bounds = {
                    let bounds = calc_bounds(&pool, &info, srid, true);
                    pin_mut!(bounds);
                    timeout(DEFAULT_BOUNDS_TIMEOUT, &mut bounds).await
                };

                if let Ok(bounds) = bounds {
                    info.bounds = bounds?;
                } else {
                    warn!(
                            "Timeout computing {} bounds for {id}, aborting query. Use --auto-bounds=calc to wait until complete, or check the table for missing indices.",
                            info.format_id(),
                        );
                }
            }
        }

        if let Some(bounds) = info.bounds {
            debug!(
                "The computed bounds for {id} from {} are {bounds}",
                info.format_id()
            );
        }
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
        // let val = EARTH_CIRCUMFERENCE * buffer as f64 / extent as f64;
        // format!("ST_Expand(ST_TileEnvelope($1::integer, $2::integer, $3::integer), {val}/2^$1::integer)")
        "ST_TileEnvelope($1::integer, $2::integer, $3::integer)".to_string()
    };

    let limit_clause = max_feature_count.map_or(String::new(), |v| format!("LIMIT {v}"));
    let layer_id = escape_literal(info.layer_id.as_ref().unwrap_or(&id));
    let clip_geom = info.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM);
    let schema = escape_identifier(&info.schema);
    let table = escape_identifier(&info.table);
    let geometry_column = escape_identifier(&info.geometry_column);
    let query = format!(
        r"
SELECT
  ST_AsMVT(tile, {layer_id}, {extent}, 'geom'{id_name})
FROM (
  SELECT
    ST_AsMVTGeom(
        ST_Transform(ST_CurveToLine({geometry_column}::geometry), 3857),
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
"
    )
    .trim()
    .to_string();

    Ok((id, PgSqlInfo::new(query, false, info.format_id()), info))
}

/// Compute the bounds of a table. This could be slow if the table is large or has no geo index.
async fn calc_bounds(
    pool: &PgPool,
    info: &TableInfo,
    srid: i32,
    mut is_quick: bool,
) -> PgResult<Option<Bounds>> {
    let schema = escape_identifier(&info.schema);
    let table = escape_identifier(&info.table);

    let cn = pool.get().await?;
    loop {
        let query = if is_quick {
            // This method is faster but less accurate, and can fail in a number of cases (returns NULL)
            cn.query_one(
                "SELECT ST_Transform(ST_SetSRID(ST_EstimatedExtent($1, $2, $3)::geometry, $4), 4326) as bounds",
                &[
                    &&schema[1..schema.len() - 1],
                    &&table[1..table.len() - 1],
                    &info.geometry_column,
                    &srid,
                ],
            ).await
        } else {
            let geometry_column = escape_identifier(&info.geometry_column);
            cn.query_one(
                &format!(r"
WITH real_bounds AS (SELECT ST_SetSRID(ST_Extent({geometry_column}::geometry), {srid}) AS rb FROM {schema}.{table})
SELECT ST_Transform(
            CASE
                WHEN (SELECT ST_GeometryType(rb) FROM real_bounds LIMIT 1) = 'ST_Point'
                THEN ST_SetSRID(ST_Extent(ST_Expand({geometry_column}::geometry, 1)), {srid})
                ELSE (SELECT * FROM real_bounds)
            END,
            4326
        ) AS bounds
FROM {schema}.{table};"),
                &[]
            ).await
        };

        if let Some(bounds) = query
            .map_err(|e| PostgresError(e, "querying table bounds"))?
            .get::<_, Option<ewkb::Polygon>>("bounds")
        {
            return Ok(polygon_to_bbox(&bounds));
        }
        if is_quick {
            // ST_EstimatedExtent failed probably because there is no index or statistics or if it's a view
            // This can only happen once if we are in quick mode
            is_quick = false;
            warn!("ST_EstimatedExtent on {schema}.{table}.{} failed, trying slower method to compute bounds", info.geometry_column);
        } else {
            return Ok(None);
        }
    }
}
