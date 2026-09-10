//! `PostgreSQL` table discovery and validation.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::num::NonZeroU32;

use futures::pin_mut;
use martin_core::tiles::postgres::PostgresError::{InvalidFilter, PostgresError};
use martin_core::tiles::postgres::{PostgresPool, PostgresResult, PostgresSqlInfo};
use martin_tile_utils::{EARTH_CIRCUMFERENCE, EARTH_CIRCUMFERENCE_DEGREES};
use postgis::ewkb;
use postgres_protocol::escape::{escape_identifier, escape_literal};
use serde_json::Value;
use tilejson::Bounds;
use tokio::time::timeout;
use tracing::{debug, warn};

use crate::config::args::{BoundsCalcType, DEFAULT_BOUNDS_TIMEOUT};
use crate::config::file::postgres::{PgTileGrid, PostgresInfo as _, TableInfo};

/// Map of `PostgreSQL` tables organized by schema, table, and geometry column.
pub type SqlTableInfoMapMapMap = BTreeMap<String, BTreeMap<String, BTreeMap<String, TableInfo>>>;

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;
const DEFAULT_CLIP_GEOM: bool = true;

/// Queries the database for available tables with geometry columns.
///
/// The reported tables are filtered by the `restrict_to_tables` parameter.
pub async fn query_available_tables(
    pool: &PostgresPool,
    restrict_to_tables: Option<HashSet<(String, String)>>,
) -> PostgresResult<SqlTableInfoMapMapMap> {
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

        // Within the config, if auto_publish is false or omitted, the list of schema and table
        // names set explicitly under the tables key is provided to the function. As the query above
        // may return more tables than explicitly defined, these are filtered out below.
        if let Some(ref table_names) = restrict_to_tables
            && !table_names.contains(&(schema.to_lowercase(), table.to_lowercase()))
        {
            continue;
        }

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
            relkind: row
                .get::<_, Option<i8>>("relkind")
                .and_then(|r| u8::try_from(r).ok().map(char::from)),
            srid: row.get("srid"), // casting i32 to u32?
            geometry_type: row.get("type"),
            properties: Some(
                serde_json::from_value(row.get("properties"))
                    .expect("properties column should be a valid JSON object with string values"),
            ),
            tilejson,
            ..Default::default()
        };

        // Warn for missing geometry indices.
        // Ignore views since those can't have indices and will generally refer to table columns.
        if info.geometry_index == Some(false) && info.relkind != Some('v') {
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
    pool: PostgresPool,
    bounds_type: BoundsCalcType,
    max_feature_count: Option<usize>,
    grid: &PgTileGrid,
) -> PostgresResult<(String, PostgresSqlInfo, TableInfo)> {
    let srid = info.srid;

    if info.bounds.is_none() && grid.grid().is_simple() {
        // plain planar units have no place in WGS84 bounds
        debug!(
            "Not computing the bounds of {id}: tile grid {} is not geographic",
            grid.grid().id()
        );
    } else if info.bounds.is_none()
        && bounds_type != BoundsCalcType::Skip
        && transforms_to_wgs84(&pool, &info, srid).await?
    {
        match bounds_type {
            BoundsCalcType::Skip => {}
            BoundsCalcType::Calc => {
                debug!("Computing {} table bounds for {id}", info.format_id());
                info.bounds = calc_bounds(&pool, &info, srid, BoundsCalcMode::Exact).await?;
            }
            BoundsCalcType::Quick => {
                debug!(
                    "Computing {} table bounds with {}s timeout for {id}",
                    info.format_id(),
                    DEFAULT_BOUNDS_TIMEOUT.as_secs()
                );
                let bounds = {
                    let bounds = calc_bounds(&pool, &info, srid, BoundsCalcMode::Estimate);
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

    let extent = info.extent.map_or(DEFAULT_EXTENT, NonZeroU32::get);
    let buffer = info.buffer.unwrap_or(DEFAULT_BUFFER);
    let margin = f64::from(buffer) / f64::from(extent);
    let geometry_column = escape_identifier(&info.geometry_column);
    // `ST_AsMVTGeom` cannot encode arcs, so only columns that may hold them are linearized.
    let geometry = if may_contain_arcs(info.geometry_type.as_deref()) {
        format!("ST_CurveToLine({geometry_column}::geometry)")
    } else {
        format!("{geometry_column}::geometry")
    };
    let table_wrap = if grid.is_web_mercator() || srid == grid.srid() {
        None
    } else {
        wrap_width(&pool, srid).await
    };
    let GridSql {
        geometry,
        envelope,
        bbox_search,
    } = grid_sql(
        grid,
        srid,
        &geometry,
        buffer,
        margin,
        pool.supports_tile_margin(),
        table_wrap,
    );

    let limit_clause = max_feature_count.map_or(String::new(), |v| format!("LIMIT {v}"));
    let filter = row_filter(&info, "AND")?;
    let layer_id = escape_literal(info.layer_id.as_ref().unwrap_or(&id));
    let clip_geom = info.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM);
    let schema = escape_identifier(&info.schema);
    let table = escape_identifier(&info.table);
    let query = format!(
        r"
SELECT
  ST_AsMVT(tile, {layer_id}, {extent}, 'geom'{id_name})
FROM (
  SELECT
    ST_AsMVTGeom(
        {geometry},
        {envelope},
        {extent}, {buffer}, {clip_geom}
    ) AS geom
    {id_field}{properties}
  FROM
    {schema}.{table}
  WHERE
    {geometry_column} && {bbox_search}{filter}
  {limit_clause}
) AS tile;
"
    )
    .trim()
    .to_owned();

    Ok((
        id,
        PostgresSqlInfo::new(
            query,
            false,
            // a table tile is empty only when no geometry intersects its envelope, which contains the envelopes of its children
            true,
            info.format_id(),
            false,
        ),
        info,
    ))
}

/// The configured CQL2 `filter` as a SQL clause starting with `keyword`, or nothing.
fn row_filter(info: &TableInfo, keyword: &str) -> PostgresResult<String> {
    use cql2::ToSqlAst as _;
    let Some(filter) = info.filter.as_deref() else {
        return Ok(String::new());
    };
    let invalid = |reason: String| InvalidFilter(filter.to_owned(), reason);
    let expr = cql2::parse_text(filter).map_err(|e| invalid(e.to_string()))?;
    let sql = expr.to_sql().map_err(|e| invalid(e.to_string()))?;
    Ok(format!(" {keyword} ({sql})"))
}

/// Whether a column of this geometry type can hold circular arcs.
/// Everything but the six linear types is assumed to, including the generic `GEOMETRY` and an unknown type.
fn may_contain_arcs(geometry_type: Option<&str>) -> bool {
    let Some(geometry_type) = geometry_type else {
        return true;
    };
    let upper = geometry_type.trim().to_ascii_uppercase();
    let base = upper
        .strip_suffix("ZM")
        .or_else(|| upper.strip_suffix('Z'))
        .or_else(|| upper.strip_suffix('M'))
        .unwrap_or(&upper);
    !matches!(
        base,
        "POINT" | "MULTIPOINT" | "LINESTRING" | "MULTILINESTRING" | "POLYGON" | "MULTIPOLYGON"
    )
}

/// The three SQL fragments of a table query that depend on the grid it is served in.
struct GridSql {
    /// The geometry, in the grid's CRS.
    geometry: String,
    /// The requested tile's envelope, in the grid's CRS.
    envelope: String,
    /// What the table's geometry column is index-searched against, in the table's CRS.
    bbox_search: String,
}

/// Builds the grid-dependent parts of a table query.
///
/// For the default Web Mercator grid this is the SQL martin has always generated.
/// Any other grid passes its zoom-0 square to `ST_TileEnvelope` as the `bounds` argument.
/// It skips the geometry transform when the table already stores the grid's CRS.
/// It densifies the envelope before transforming it into the table's CRS, so that edges which curve in that CRS still cover the tile.
/// `table_wrap` is the width of the world in the table's CRS when that CRS cuts the world open at the antimeridian, see [`wrap_width`].
fn grid_sql(
    grid: &PgTileGrid,
    table_srid: i32,
    geometry: &str,
    buffer: u32,
    margin: f64,
    supports_tile_margin: bool,
    table_wrap: Option<f64>,
) -> GridSql {
    const TILE: &str = "$1::integer, $2::integer, $3::integer";
    if grid.is_web_mercator() {
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
            format!("ST_Transform(ST_TileEnvelope({TILE}), {table_srid})")
        } else if supports_tile_margin && table_srid == 3857 {
            format!("ST_Transform(ST_TileEnvelope({TILE}, margin => {margin}), {table_srid})")
        } else if table_srid == 4326 {
            format!(
                "ST_Expand(ST_Transform(ST_TileEnvelope({TILE}), {table_srid}), ({margin} * {EARTH_CIRCUMFERENCE_DEGREES}) / 2^$1::integer)"
            )
        } else {
            format!("ST_Transform(ST_TileEnvelope({TILE}), {table_srid})")
        };
        return GridSql {
            geometry: format!("ST_Transform({geometry}, 3857)"),
            envelope: format!("ST_TileEnvelope({TILE})"),
            bbox_search,
        };
    }

    let grid_srid = grid.srid();
    let [x0, y0] = grid.grid().origin();
    let extent_at_zoom0 = grid.grid().extent_at_zoom0();
    // the zoom-0 square, spelled with the configured numbers so the query reads like the config
    let envelope = if grid.grid().matrix_at_zoom0() == [1, 1] {
        format!(
            "ST_TileEnvelope({TILE}, ST_MakeEnvelope({x0}, {y0} - {extent_at_zoom0}, {x0} + {extent_at_zoom0}, {y0}, {grid_srid}))"
        )
    } else {
        // two tiles at zoom 0 are one zoom level of a square twice the size, anchored at the same corner
        format!(
            "ST_TileEnvelope($1::integer + 1, $2::integer, $3::integer, ST_MakeEnvelope({x0}, {y0} - 2 * {extent_at_zoom0}, {x0} + 2 * {extent_at_zoom0}, {y0}, {grid_srid}))"
        )
    };
    let geometry = if table_srid == grid_srid {
        geometry.to_owned()
    } else {
        format!("ST_Transform({geometry}, {grid_srid})")
    };
    // the margin is applied in grid units, like the 4326 branch above does in degrees
    let search = if buffer == 0 {
        envelope.clone()
    } else {
        format!("ST_Expand({envelope}, ({margin} * {extent_at_zoom0}) / 2^$1::integer)")
    };
    // a transformed envelope is only as good as its vertices: eight per side keeps curved edges covered
    let bbox_search = if table_srid == grid_srid {
        search
    } else {
        let transformed = format!(
            "ST_Transform(ST_Segmentize({search}, {extent_at_zoom0} / 2^$1::integer / 8), {table_srid})"
        );
        match table_wrap {
            // a tile across the cut has vertices at both ends of the world once transformed, so its
            // bounding box lands on the wrong side and misses the strip next to the cut; search the
            // whole width of the world at the tile's height instead, and let ST_AsMVTGeom clip
            Some(width) => {
                let half = width / 2.0;
                format!(
                    "(SELECT CASE WHEN ST_XMax(search) - ST_XMin(search) > {half} THEN ST_MakeEnvelope(-{half}, ST_YMin(search), {half}, ST_YMax(search), {table_srid}) ELSE search END FROM (SELECT {transformed} AS search) AS s)"
                )
            }
            None => transformed,
        }
    };
    GridSql {
        geometry,
        envelope,
        bbox_search,
    }
}

/// How [`calc_bounds`] should compute a table's geometry bounds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundsCalcMode {
    /// Exact `ST_Extent` over the whole table. Accurate, but potentially slow on large or unindexed tables.
    Exact,
    /// Fast `ST_EstimatedExtent` from table statistics, falling back to [`Self::Exact`] when unavailable.
    Estimate,
}

/// Compute the bounds of a table. This could be slow if the table is large or has no geo index.
async fn calc_bounds(
    pool: &PostgresPool,
    info: &TableInfo,
    srid: i32,
    mode: BoundsCalcMode,
) -> PostgresResult<Option<Bounds>> {
    let schema = escape_identifier(&info.schema);
    let table = escape_identifier(&info.table);
    let cn = pool.get().await?;

    // Table statistics cover every row, so a filtered source always measures its rows.
    if mode == BoundsCalcMode::Estimate && info.filter.is_none() {
        // ST_EstimatedExtent reads the index/statistics instead of scanning the table, and matches
        // its arguments against the catalog by raw (unescaped) name. A degenerate point/line
        // estimate is expanded into a polygon, like the exact calculation below. Any failure (an
        // unparseable name, no index/statistics, a view, or a non-polygon result) falls back to the
        // exact calculation rather than aborting.
        let estimate = cn
            .query_one(
                r"
SELECT ST_Transform(
            ST_SetSRID(
                CASE
                    WHEN ST_GeometryType(ext) IN ('ST_Point', 'ST_LineString')
                    THEN ST_Envelope(ST_Expand(ext, 1))
                    ELSE ext
                END,
                $4),
            4326) AS bounds
FROM (SELECT ST_EstimatedExtent($1, $2, $3)::geometry AS ext) AS estimate;",
                &[&info.schema, &info.table, &info.geometry_column, &srid],
            )
            .await
            .ok()
            .and_then(|row| {
                row.try_get::<_, Option<ewkb::Polygon>>("bounds")
                    .ok()
                    .flatten()
            });
        if let Some(bounds) = estimate {
            return Ok(polygon_to_bbox(&bounds));
        }
        warn!(
            "ST_EstimatedExtent on {schema}.{table}.{} failed, trying slower method to compute bounds",
            info.geometry_column
        );
    }

    let geometry_column = escape_identifier(&info.geometry_column);
    let filter = row_filter(info, "WHERE")?;
    Ok(cn
        .query_one(
            &format!(r"
WITH real_bounds AS (SELECT ST_SetSRID(ST_Extent({geometry_column}::geometry), {srid}) AS rb FROM {schema}.{table}{filter})
SELECT ST_Transform(
            CASE
                WHEN (SELECT ST_GeometryType(rb) FROM real_bounds LIMIT 1) IN ('ST_Point', 'ST_LineString')
                THEN ST_SetSRID(ST_Extent(ST_Expand({geometry_column}::geometry, 1)), {srid})
                ELSE (SELECT * FROM real_bounds)
            END,
            4326
        ) AS bounds
FROM {schema}.{table}{filter};"),
            &[],
        )
        .await
        .map_err(|e| PostgresError(e, "querying table bounds"))?
        .get::<_, Option<ewkb::Polygon>>("bounds")
        .and_then(|p| polygon_to_bbox(&p)))
}

/// Whether `PostGIS` can transform `srid` into WGS84, which `TileJSON` bounds are expressed in.
///
/// EPSG systems always can. A CRS from another authority, such as a planetary one, need not map onto WGS84 at all.
/// That table can still be served, it just cannot advertise bounds, and the warning says so.
/// The probe transforms an empty geometry, so the answer never waits on a table scan or a bounds timeout.
/// A failed projection leaves `PostGIS` state behind that breaks later transforms on the same connection,
/// so that connection is closed instead of being returned to the pool.
async fn transforms_to_wgs84(
    pool: &PostgresPool,
    info: &TableInfo,
    srid: i32,
) -> PostgresResult<bool> {
    let Some(authority) = non_epsg_authority(pool, srid).await else {
        return Ok(true);
    };
    let cn = pool.get().await?;
    let probe = cn
        .query_one(
            "SELECT ST_Transform(ST_GeomFromText('POINT EMPTY', $1), 4326)",
            &[&srid],
        )
        .await;
    let Err(e) = probe else {
        return Ok(true);
    };
    PostgresPool::discard(cn);
    let reason = e
        .as_db_error()
        .map_or_else(|| e.to_string(), |db| db.message().to_owned());
    warn!(
        "Not computing the bounds of {}: SRID {srid} is {authority}, not an EPSG system, so PostGIS cannot express them in WGS84 ({reason}). Set bounds in the config to advertise them.",
        info.format_id()
    );
    Ok(false)
}

/// The `AUTHORITY:CODE` of `srid` when `spatial_ref_sys` knows it under an authority other than EPSG.
async fn non_epsg_authority(pool: &PostgresPool, srid: i32) -> Option<String> {
    let row = pool
        .get()
        .await
        .ok()?
        .query_opt(
            "SELECT auth_name, auth_srid FROM spatial_ref_sys WHERE srid = $1",
            &[&srid],
        )
        .await
        .ok()??;
    let authority: Option<String> = row.get("auth_name");
    let code: Option<i32> = row.get("auth_srid");
    let authority = authority?;
    if authority.eq_ignore_ascii_case("EPSG") {
        return None;
    }
    Some(code.map_or_else(|| authority.clone(), |code| format!("{authority}:{code}")))
}

/// The width of the world in the units of `srid`, when that CRS cuts the world open at the antimeridian.
///
/// Geographic systems are 360 degrees wide and Web Mercator one earth circumference.
/// Other systems with such a cut are not recognised, and a database failure counts as "no cut", since this only widens a search.
async fn wrap_width(pool: &PostgresPool, srid: i32) -> Option<f64> {
    match srid {
        3857 => return Some(EARTH_CIRCUMFERENCE),
        4326 => return Some(f64::from(EARTH_CIRCUMFERENCE_DEGREES)),
        _ => {}
    }
    let row = pool
        .get()
        .await
        .ok()?
        .query_opt(
            "SELECT proj4text LIKE '%+proj=longlat%' OR srtext LIKE 'GEOGC%' FROM spatial_ref_sys WHERE srid = $1",
            &[&srid],
        )
        .await
        .ok()??;
    row.get::<_, Option<bool>>(0)
        .unwrap_or(false)
        .then_some(f64::from(EARTH_CIRCUMFERENCE_DEGREES))
}

#[must_use]
pub fn polygon_to_bbox(polygon: &ewkb::Polygon) -> Option<Bounds> {
    use postgis::{LineString as _, Point as _, Polygon as _};

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

#[cfg(test)]
mod tests {
    use martin_tile_utils::TileGrid;
    use rstest::rstest;

    use super::*;

    const MARGIN: f64 = 64.0 / 4096.0;

    fn nztm2000quad() -> PgTileGrid {
        PgTileGrid::new(
            TileGrid::new(
                "NZTM2000Quad",
                "EPSG:2193",
                [-3_260_586.728_4, 10_438_190.165_2],
                10_018_754.171_4,
            )
            .unwrap(),
            2193,
        )
    }

    /// The default grid keeps producing the SQL martin generated before grids existed, byte for byte.
    #[rstest]
    #[case::mercator_table_with_margin(3857, 64, true,
        r"ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer, margin => 0.015625), 3857)")]
    #[case::mercator_table_old_postgis(
        3857,
        64,
        false,
        r"ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer), 3857)"
    )]
    #[case::wgs84_table(4326, 64, true,
        r"ST_Expand(ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer), 4326), (0.015625 * 360) / 2^$1::integer)")]
    #[case::other_table(
        25832,
        64,
        true,
        r"ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer), 25832)"
    )]
    #[case::no_buffer(
        4326,
        0,
        true,
        r"ST_Transform(ST_TileEnvelope($1::integer, $2::integer, $3::integer), 4326)"
    )]
    fn web_mercator_sql_is_unchanged(
        #[case] table_srid: i32,
        #[case] buffer: u32,
        #[case] supports_tile_margin: bool,
        #[case] bbox_search: &str,
    ) {
        let sql = grid_sql(
            &PgTileGrid::web_mercator(),
            table_srid,
            "ST_CurveToLine(\"geom\"::geometry)",
            buffer,
            MARGIN,
            supports_tile_margin,
            None,
        );
        assert_eq!(
            sql.geometry,
            r#"ST_Transform(ST_CurveToLine("geom"::geometry), 3857)"#
        );
        assert_eq!(
            sql.envelope,
            "ST_TileEnvelope($1::integer, $2::integer, $3::integer)"
        );
        assert_eq!(sql.bbox_search, bbox_search);
    }

    #[test]
    fn a_table_stored_in_the_grid_crs_is_never_transformed() {
        let sql = grid_sql(
            &nztm2000quad(),
            2193,
            "ST_CurveToLine(\"geom\"::geometry)",
            64,
            MARGIN,
            true,
            None,
        );
        insta::assert_snapshot!(sql.geometry, @r#"ST_CurveToLine("geom"::geometry)"#);
        insta::assert_snapshot!(sql.envelope, @"ST_TileEnvelope($1::integer, $2::integer, $3::integer, ST_MakeEnvelope(-3260586.7284, 10438190.1652 - 10018754.1714, -3260586.7284 + 10018754.1714, 10438190.1652, 2193))");
        insta::assert_snapshot!(sql.bbox_search, @"ST_Expand(ST_TileEnvelope($1::integer, $2::integer, $3::integer, ST_MakeEnvelope(-3260586.7284, 10438190.1652 - 10018754.1714, -3260586.7284 + 10018754.1714, 10438190.1652, 2193)), (0.015625 * 10018754.1714) / 2^$1::integer)");
    }

    #[test]
    fn a_table_in_another_crs_is_transformed_and_searched_through_a_densified_envelope() {
        let sql = grid_sql(
            &nztm2000quad(),
            27700,
            "ST_CurveToLine(\"geom\"::geometry)",
            64,
            MARGIN,
            true,
            None,
        );
        insta::assert_snapshot!(sql.geometry, @r#"ST_Transform(ST_CurveToLine("geom"::geometry), 2193)"#);
        insta::assert_snapshot!(sql.bbox_search, @"ST_Transform(ST_Segmentize(ST_Expand(ST_TileEnvelope($1::integer, $2::integer, $3::integer, ST_MakeEnvelope(-3260586.7284, 10438190.1652 - 10018754.1714, -3260586.7284 + 10018754.1714, 10438190.1652, 2193)), (0.015625 * 10018754.1714) / 2^$1::integer), 10018754.1714 / 2^$1::integer / 8), 27700)");
    }

    /// `NZTM2000Quad` reaches across 180 degrees, where longitude and latitude cut the world open.
    /// A tile there has vertices at both ends of the world once transformed, so the search falls back to the whole width of the world at the tile's height.
    #[test]
    fn a_table_in_a_crs_with_an_antimeridian_cut_is_searched_across_the_whole_width_when_the_tile_crosses_it()
     {
        let sql = grid_sql(
            &nztm2000quad(),
            4326,
            "ST_CurveToLine(\"geom\"::geometry)",
            64,
            MARGIN,
            true,
            Some(360.0),
        );
        insta::assert_snapshot!(sql.bbox_search, @"(SELECT CASE WHEN ST_XMax(search) - ST_XMin(search) > 180 THEN ST_MakeEnvelope(-180, ST_YMin(search), 180, ST_YMax(search), 4326) ELSE search END FROM (SELECT ST_Transform(ST_Segmentize(ST_Expand(ST_TileEnvelope($1::integer, $2::integer, $3::integer, ST_MakeEnvelope(-3260586.7284, 10438190.1652 - 10018754.1714, -3260586.7284 + 10018754.1714, 10438190.1652, 2193)), (0.015625 * 10018754.1714) / 2^$1::integer), 10018754.1714 / 2^$1::integer / 8), 4326) AS search) AS s)");
    }

    #[test]
    fn two_tiles_at_zoom0_are_one_zoom_of_a_double_square() {
        let grid = PgTileGrid::new(martin_tile_utils::WORLD_CRS84_QUAD, 4326);
        let sql = grid_sql(&grid, 4326, "\"geom\"", 64, MARGIN, true, None);
        insta::assert_snapshot!(sql.envelope, @"ST_TileEnvelope($1::integer + 1, $2::integer, $3::integer, ST_MakeEnvelope(-180, 90 - 2 * 180, -180 + 2 * 180, 90, 4326))");
        insta::assert_snapshot!(sql.bbox_search, @"ST_Expand(ST_TileEnvelope($1::integer + 1, $2::integer, $3::integer, ST_MakeEnvelope(-180, 90 - 2 * 180, -180 + 2 * 180, 90, 4326)), (0.015625 * 180) / 2^$1::integer)");
    }

    #[test]
    fn a_simple_grid_needs_no_transform_at_all() {
        let plan = TileGrid::new(
            "floor",
            martin_tile_utils::SIMPLE_CRS,
            [0.0, 1000.0],
            1000.0,
        )
        .unwrap();
        let sql = grid_sql(
            &PgTileGrid::new(plan, 0),
            0,
            "ST_CurveToLine(\"geom\"::geometry)",
            0,
            0.0,
            true,
            None,
        );
        insta::assert_snapshot!(sql.geometry, @r#"ST_CurveToLine("geom"::geometry)"#);
        insta::assert_snapshot!(sql.envelope, @"ST_TileEnvelope($1::integer, $2::integer, $3::integer, ST_MakeEnvelope(0, 1000 - 1000, 0 + 1000, 1000, 0))");
        insta::assert_snapshot!(sql.bbox_search, @"ST_TileEnvelope($1::integer, $2::integer, $3::integer, ST_MakeEnvelope(0, 1000 - 1000, 0 + 1000, 1000, 0))");
    }

    #[test]
    fn no_buffer_means_no_expansion() {
        let sql = grid_sql(
            &nztm2000quad(),
            4326,
            "ST_CurveToLine(\"geom\"::geometry)",
            0,
            0.0,
            true,
            None,
        );
        insta::assert_snapshot!(sql.bbox_search, @"ST_Transform(ST_Segmentize(ST_TileEnvelope($1::integer, $2::integer, $3::integer, ST_MakeEnvelope(-3260586.7284, 10438190.1652 - 10018754.1714, -3260586.7284 + 10018754.1714, 10438190.1652, 2193)), 10018754.1714 / 2^$1::integer / 8), 4326)");
    }
}
