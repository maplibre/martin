//! `PostgreSQL` table discovery and validation.

use std::collections::{BTreeMap, HashMap};

use futures::pin_mut;
use log::{debug, warn};
use martin_core::tiles::postgres::PostgresError::PostgresError;
use martin_core::tiles::postgres::{PostgresPool, PostgresResult, PostgresSqlInfo};
use martin_tile_utils::EARTH_CIRCUMFERENCE_DEGREES;
use postgis::ewkb;
use postgres_protocol::escape::{escape_identifier, escape_literal};
use serde_json::Value;
use tilejson::Bounds;
use tokio::time::timeout;

use crate::config::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::file::postgres::{PostgresInfo, TableInfo};

/// Map of `PostgreSQL` tables organized by schema, table, and geometry column.
pub type SqlTableInfoMapMapMap = BTreeMap<String, BTreeMap<String, BTreeMap<String, TableInfo>>>;

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;
const DEFAULT_CLIP_GEOM: bool = true;

/// Queries the database for available tables with geometry columns.
pub async fn query_available_tables(pool: &PostgresPool) -> PostgresResult<SqlTableInfoMapMapMap> {
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
                    warn!(
                        "Unable to deserialize SQL comment on {schema}.{table} as tilejson, the automatically generated tilejson would be used: {e}"
                    );
                    None
                }
            }
        } else {
            debug!(
                "Unable to find a  SQL comment on {schema}.{table}, the tilejson would be generated automatically"
            );
            None
        };

        let info = TableInfo {
            schema,
            table,
            geometry_column: row.get("geom"),
            geometry_index: row.get("geom_idx"),
            relkind: row.get("relkind"),
            srid: row.get("srid"), // casting i32 to u32?
            geometry_type: row.get("type"),
            properties: Some(serde_json::from_value(row.get("properties")).unwrap()),
            tilejson,
            ..Default::default()
        };

        // Warn for missing geometry indices.
        // Ignore views since those can't have indices and will generally refer to table columns.
        if info.geometry_index == Some(false) && info.relkind.as_deref() != Some("v") {
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

/// Generate a query to fetch tiles from a table.
/// The function is async because it may need to query the database for the table bounds (could be very slow).
pub async fn table_to_query(
    id: String,
    mut info: TableInfo,
    pool: PostgresPool,
    bounds_type: BoundsCalcType,
    max_feature_count: Option<usize>,
) -> PostgresResult<(String, PostgresSqlInfo, TableInfo)> {
    let schema = escape_identifier(&info.schema);
    let table = escape_identifier(&info.table);
    let geometry_column = escape_identifier(&info.geometry_column);
    let srid = info.srid;

    if info.bounds.is_none() {
        match bounds_type {
            BoundsCalcType::Skip => {}
            BoundsCalcType::Calc => {
                debug!("Computing {} table bounds for {id}", info.format_id());
                info.bounds = calc_bounds(&pool, &schema, &table, &geometry_column, srid).await?;
            }
            BoundsCalcType::Quick => {
                debug!(
                    "Computing {} table bounds with {}s timeout for {id}",
                    info.format_id(),
                    DEFAULT_BOUNDS_TIMEOUT.as_secs()
                );
                let bounds = calc_bounds(&pool, &schema, &table, &geometry_column, srid);
                pin_mut!(bounds);
                if let Ok(bounds) = timeout(DEFAULT_BOUNDS_TIMEOUT, &mut bounds).await {
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
    let margin = f64::from(buffer) / f64::from(extent);

    // When calculating the bounding box to search within, a few considerations must be made when
    // using a margin. The ST_TileEnvelope margin parameter is for use with SRID 3857.
    // For SRID 4326, ST_Expand is used and provided with SRID 4326 specific units (degrees).
    // If the table uses a non-standard SRID, it will fall back to existing behavior.
    //
    // For more context, if SRID 4326 were to be used with ST_TileEnvelope and margin
    // parameter, the resultant bounding box for tiles on the antimeridian would be calculated
    // incorrectly. For example, with a margin of 2 units, the antimeridian edge would transform
    // from -180 to +178. This results in a bbox that stretches from the easternmost edge of a tile
    // (plus margin) around the map to the westernmost edge of the tile (minus margin). The
    // resulting bbox covers none of the original tile. In contrast, for this example, ST_Expand
    // will result in a westernmost edge (minus margin) of -182.
    let bbox_search = if buffer == 0 {
        format!("ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer), {srid})")
    } else if pool.supports_tile_margin() && srid == 3857 {
        format!(
            "ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer, margin => {margin}), {srid})"
        )
    } else if srid == 4326 {
        format!(
            "ST_Expand(ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer), {srid}), ({margin} * {EARTH_CIRCUMFERENCE_DEGREES}) / 2^$1::integer)"
        )
    } else {
        format!("ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer), {srid})")
    };

    let limit_clause = max_feature_count.map_or(String::new(), |v| format!("LIMIT {v}"));
    let layer_id = escape_literal(info.layer_id.as_ref().unwrap_or(&id));
    let clip_geom = info.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM);
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
    {geometry_column} && {bbox_search}
  {limit_clause}
) AS tile;
"
    )
    .trim()
    .to_string();

    Ok((
        id,
        PostgresSqlInfo::new(query, false, info.format_id()),
        info,
    ))
}

/// Compute the bounds of a table. This could be slow if the table is large or has no geo index.
async fn calc_bounds(
    pool: &PostgresPool,
    schema: &str,
    table: &str,
    geometry_column: &str,
    srid: i32,
) -> PostgresResult<Option<Bounds>> {
    Ok(pool.get()
        .await?
        .query_one(&format!(
            r"
WITH real_bounds AS (SELECT ST_SetSRID(ST_Extent({geometry_column}::geometry), {srid}) AS rb FROM {schema}.{table})
SELECT ST_Transform(
            CASE
                WHEN (SELECT ST_GeometryType(rb) FROM real_bounds LIMIT 1) = 'ST_Point'
                THEN ST_SetSRID(ST_Extent(ST_Expand({geometry_column}::geometry, 1)), {srid})
                ELSE (SELECT * FROM real_bounds)
            END,
            4326
        ) AS bounds
FROM {schema}.{table};
                "), &[])
        .await
        .map_err(|e| PostgresError(e, "querying table bounds"))?
        .get::<_, Option<ewkb::Polygon>>("bounds")
        .and_then(|p| polygon_to_bbox(&p)))
}

#[must_use]
pub fn polygon_to_bbox(polygon: &ewkb::Polygon) -> Option<Bounds> {
    use postgis::{LineString, Point, Polygon};

    polygon.rings().next().and_then(|linestring| {
        let mut points = linestring.points();
        if let (Some(bottom_left), Some(top_right)) = (points.next(), points.nth(1)) {
            Some(Bounds::new(
                bottom_left.x(),
                bottom_left.y(),
                top_right.x(),
                top_right.y(),
            ))
        } else {
            None
        }
    })
}
