use std::num::NonZeroU32;

use martin_tile_utils::EARTH_CIRCUMFERENCE;

use crate::config::file::tiles::duckdb::resolver::geoparquet::introspect::GeoParquetIntrospection;
use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;
use crate::config::file::tiles::duckdb::sql_utils::{
    epsg_crs, escape_identifier, escape_sql_string,
};

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;
const DEFAULT_CLIP_GEOM: bool = true;

#[must_use]
pub fn build_mvt_sql(
    introspection: &GeoParquetIntrospection,
    entry: &GeoParquetEntry,
    source_id: &str,
    from_expr: &str,
) -> String {
    let extent = entry.extent.map_or(DEFAULT_EXTENT, NonZeroU32::get);
    let buffer = entry.buffer.unwrap_or(DEFAULT_BUFFER);
    let clip_geom = entry.clip_geom.unwrap_or(DEFAULT_CLIP_GEOM);
    let margin = f64::from(buffer) / f64::from(extent);
    let source_crs = epsg_crs(introspection.srid.get());
    let target_crs = epsg_crs(3857);

    let escaped_geometry_column = escape_identifier(&introspection.geometry_column);
    // GeoParquet round-trips often drop embedded CRS metadata; stamp the resolved SRID
    // before any spatial predicate or transform.
    let source_geometry = format!("ST_SetCRS({escaped_geometry_column}::GEOMETRY, {source_crs})");
    let transformed_geometry =
        format!("ST_Transform({source_geometry}, {source_crs}, {target_crs}, always_xy := true)");
    let layer_id = escape_sql_string(entry.layer_id.as_deref().unwrap_or(source_id));

    let tile_filter = if buffer == 0 {
        format!("ST_Intersects({transformed_geometry}, bounds.envelope)")
    } else {
        format!(
            "ST_Intersects({transformed_geometry}, ST_Expand(bounds.envelope, (({margin})::DOUBLE * ({EARTH_CIRCUMFERENCE})::DOUBLE) / power(2, tile.z)))"
        )
    };

    let properties = introspection
        .property_columns
        .keys()
        .map(|column| format!(", {}", escape_identifier(column)))
        .collect::<String>();

    let (id_name, id_field) = if let Some(id_column) = &entry.id_column {
        (
            format!(", {}", escape_sql_string(id_column)),
            format!(", {}", escape_identifier(id_column)),
        )
    } else {
        (String::new(), String::new())
    };

    format!(
        r"
WITH tile AS (
    SELECT
        ?::INTEGER AS z,
        ?::INTEGER AS x,
        ?::INTEGER AS y
),
bounds AS (
    SELECT ST_TileEnvelope(tile.z, tile.x, tile.y) AS envelope
    FROM tile
)
SELECT ST_AsMVT(tile, {layer_id}, {extent}, 'geom'{id_name})
FROM (
  SELECT
    ST_AsMVTGeom(
        {transformed_geometry},
        ST_Extent(ST_TileEnvelope(tile.z, tile.x, tile.y)),
        {extent}::BIGINT, {buffer}::BIGINT, {clip_geom}
    ) AS geom
    {id_field}{properties}
  FROM {from_expr}, tile, bounds
  WHERE {tile_filter}
) AS tile;
"
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroI32;

    use super::*;
    use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;
    use crate::config::file::tiles::duckdb::sql_utils::escape_sql_string;

    fn introspection_with_srid(srid: i32) -> GeoParquetIntrospection {
        GeoParquetIntrospection {
            geometry_column: "geom".to_string(),
            srid: NonZeroI32::new(srid).expect("test srid is non-zero"),
            property_columns: BTreeMap::from([
                ("name".to_string(), "VARCHAR".to_string()),
                ("category".to_string(), "VARCHAR".to_string()),
            ]),
        }
    }

    #[test]
    fn build_mvt_sql_includes_core_fragments() {
        let introspection = introspection_with_srid(4326);
        let from_expr = format!("read_parquet({})", escape_sql_string("/data/points.parquet"));
        let sql = build_mvt_sql(
            &introspection,
            &GeoParquetEntry::default(),
            "buildings",
            &from_expr,
        );

        insta::assert_snapshot!(sql, @r#"
        WITH tile AS (
            SELECT
                ?::INTEGER AS z,
                ?::INTEGER AS x,
                ?::INTEGER AS y
        ),
        bounds AS (
            SELECT ST_TileEnvelope(tile.z, tile.x, tile.y) AS envelope
            FROM tile
        )
        SELECT ST_AsMVT(tile, 'buildings', 4096, 'geom')
        FROM (
          SELECT
            ST_AsMVTGeom(
                ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true),
                ST_Extent(ST_TileEnvelope(tile.z, tile.x, tile.y)),
                4096::BIGINT, 64::BIGINT, true
            ) AS geom
            , "category", "name"
          FROM read_parquet('/data/points.parquet'), tile, bounds
          WHERE ST_Intersects(ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true), ST_Expand(bounds.envelope, ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, tile.z)))
        ) AS tile;
        "#);
    }

    #[test]
    fn build_mvt_sql_expands_bounds_for_buffered_non_wgs84_sources() {
        let from_expr = format!("read_parquet({})", escape_sql_string("/data/points.parquet"));
        let sql = build_mvt_sql(
            &introspection_with_srid(3857),
            &GeoParquetEntry::default(),
            "buildings",
            &from_expr,
        );

        insta::assert_snapshot!(sql, @r#"
        WITH tile AS (
            SELECT
                ?::INTEGER AS z,
                ?::INTEGER AS x,
                ?::INTEGER AS y
        ),
        bounds AS (
            SELECT ST_TileEnvelope(tile.z, tile.x, tile.y) AS envelope
            FROM tile
        )
        SELECT ST_AsMVT(tile, 'buildings', 4096, 'geom')
        FROM (
          SELECT
            ST_AsMVTGeom(
                ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:3857'), 'EPSG:3857', 'EPSG:3857', always_xy := true),
                ST_Extent(ST_TileEnvelope(tile.z, tile.x, tile.y)),
                4096::BIGINT, 64::BIGINT, true
            ) AS geom
            , "category", "name"
          FROM read_parquet('/data/points.parquet'), tile, bounds
          WHERE ST_Intersects(ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:3857'), 'EPSG:3857', 'EPSG:3857', always_xy := true), ST_Expand(bounds.envelope, ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, tile.z)))
        ) AS tile;
        "#);
    }

    #[test]
    fn build_mvt_sql_skips_bounds_expansion_when_buffer_is_zero() {
        let from_expr = format!("read_parquet({})", escape_sql_string("/data/points.parquet"));
        let entry = GeoParquetEntry {
            buffer: Some(0),
            ..GeoParquetEntry::default()
        };
        let sql = build_mvt_sql(
            &introspection_with_srid(4326),
            &entry,
            "buildings",
            &from_expr,
        );

        insta::assert_snapshot!(sql, @r#"
        WITH tile AS (
            SELECT
                ?::INTEGER AS z,
                ?::INTEGER AS x,
                ?::INTEGER AS y
        ),
        bounds AS (
            SELECT ST_TileEnvelope(tile.z, tile.x, tile.y) AS envelope
            FROM tile
        )
        SELECT ST_AsMVT(tile, 'buildings', 4096, 'geom')
        FROM (
          SELECT
            ST_AsMVTGeom(
                ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true),
                ST_Extent(ST_TileEnvelope(tile.z, tile.x, tile.y)),
                4096::BIGINT, 0::BIGINT, true
            ) AS geom
            , "category", "name"
          FROM read_parquet('/data/points.parquet'), tile, bounds
          WHERE ST_Intersects(ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true), bounds.envelope)
        ) AS tile;
        "#);
    }
}
