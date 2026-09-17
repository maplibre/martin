use std::num::NonZeroU32;

use martin_tile_utils::EARTH_CIRCUMFERENCE;
use tracing::debug;

use crate::config::file::tiles::duckdb::resolver::geoparquet::introspect::GeoParquetIntrospection;
use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;
use crate::config::file::tiles::duckdb::sql_utils::{
    epsg_crs, escape_identifier, escape_sql_string,
};

const DEFAULT_EXTENT: u32 = 4096;
const DEFAULT_BUFFER: u32 = 64;
const DEFAULT_CLIP_GEOM: bool = true;

/// The tile coordinates, as named parameters bound once per request.
///
/// They are written inline rather than joined in from a CTE on purpose. A CTE holding the tile
/// coordinates makes the envelope non-constant at plan time, and `DuckDB` then cannot push the
/// covering comparison into the Parquet reader, which is where all of the pruning happens.
/// Inline, `DuckDB` folds the whole envelope expression and skips row groups on their statistics.
const TILE_ENVELOPE: &str = "ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER)";

/// SRIDs whose transform to and from Web Mercator is separable and monotone per axis, so the
/// bounding box of a transformed tile envelope is exact rather than an inner approximation.
///
/// The covering predicate is a pre-filter that has to be a superset of what `ST_Intersects`
/// accepts. Under a projection with curved parallels or meridians, transforming only the four
/// corners of a tile can under-cover the true footprint and silently clip features near tile
/// edges, so those sources keep scanning every row group instead.
const AXIS_MONOTONE_SRIDS: [i32; 2] = [3857, 4326];

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

    let buffered_envelope = if buffer == 0 {
        TILE_ENVELOPE.to_owned()
    } else {
        format!(
            "ST_Expand({TILE_ENVELOPE}, (({margin})::DOUBLE * ({EARTH_CIRCUMFERENCE})::DOUBLE) / power(2, $z::INTEGER))"
        )
    };

    let mut filters = covering_filters(introspection, &buffered_envelope, source_id);
    filters.push(format!(
        "ST_Intersects({transformed_geometry}, {buffered_envelope})"
    ));
    let where_clause = filters.join("\n    AND ");

    let properties = introspection
        .property_columns
        .iter()
        .map(|(column, mvt_type)| {
            let escaped = escape_identifier(column);
            format!(", {escaped}::{mvt_type} AS {escaped}")
        })
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
SELECT ST_AsMVT(tile, {layer_id}, {extent}, 'geom'{id_name})
FROM (
  SELECT
    ST_AsMVTGeom(
        {transformed_geometry},
        ST_Extent({TILE_ENVELOPE}),
        {extent}::BIGINT, {buffer}::BIGINT, {clip_geom}
    ) AS geom
    {id_field}{properties}
  FROM {from_expr}
  WHERE {where_clause}
) AS tile;
"
    )
}

/// A four-way one-sided overlap test against the file's covering bounding box.
///
/// This is a conservative superset of `ST_Intersects`, which stays in the query as the exact
/// filter. Each clause prunes on a different statistic - `xmin <= C` on a row group's
/// `min(xmin)`, `xmax >= C` on its `max(xmax)` - so together they skip every row group whose
/// features all fall outside the tile.
///
/// Testing whether the covering box is *contained* in the tile would prune harder and be wrong:
/// that is `ST_Within`, and it drops every feature straddling a tile edge or larger than the
/// tile. The failure is invisible on point layers, where the two agree.
fn covering_filters(
    introspection: &GeoParquetIntrospection,
    buffered_envelope: &str,
    source_id: &str,
) -> Vec<String> {
    let Some(covering) = &introspection.covering else {
        return Vec::new();
    };

    let srid = introspection.srid.get();
    if !AXIS_MONOTONE_SRIDS.contains(&srid) {
        debug!(
            source.id = %source_id,
            srid,
            "Skipping GeoParquet covering pruning: a tile envelope cannot be transformed into this SRID without possibly under-covering the tile"
        );
        return Vec::new();
    }

    let source_envelope = if srid == 3857 {
        format!("ST_Extent({buffered_envelope})")
    } else {
        // The buffer pushes edge tiles past the Web Mercator domain, where proj wraps the
        // coordinate around the antimeridian instead of failing. That silently turns the
        // envelope inside out and the tile comes back empty, so clip to the world first.
        format!(
            "ST_Extent(ST_Transform(ST_Intersection({buffered_envelope}, ST_TileEnvelope(0, 0, 0)), {}, {}, always_xy := true))",
            epsg_crs(3857),
            epsg_crs(srid)
        )
    };

    vec![
        format!("{} <= ST_XMax({source_envelope})", covering.xmin),
        format!("{} >= ST_XMin({source_envelope})", covering.xmax),
        format!("{} <= ST_YMax({source_envelope})", covering.ymin),
        format!("{} >= ST_YMin({source_envelope})", covering.ymax),
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::num::NonZeroI32;

    use super::*;
    use crate::config::file::tiles::duckdb::resolver::geoparquet::covering::CoveringBbox;
    use crate::config::file::tiles::duckdb::sources::GeoParquetEntry;
    use crate::config::file::tiles::duckdb::sql_utils::escape_sql_string;

    fn introspection_with_srid(srid: i32) -> GeoParquetIntrospection {
        GeoParquetIntrospection {
            geometry_column: "geom".to_owned(),
            srid: NonZeroI32::new(srid).expect("test srid is non-zero"),
            property_columns: BTreeMap::from([
                ("name".to_owned(), "VARCHAR".to_owned()),
                ("category".to_owned(), "VARCHAR".to_owned()),
            ]),
            covering: None,
        }
    }

    fn introspection_with_covering(srid: i32) -> GeoParquetIntrospection {
        GeoParquetIntrospection {
            covering: Some(CoveringBbox {
                xmin: r#""bbox"."xmin""#.to_owned(),
                ymin: r#""bbox"."ymin""#.to_owned(),
                xmax: r#""bbox"."xmax""#.to_owned(),
                ymax: r#""bbox"."ymax""#.to_owned(),
            }),
            ..introspection_with_srid(srid)
        }
    }

    fn from_expr() -> String {
        format!(
            "read_parquet({})",
            escape_sql_string("/data/points.parquet")
        )
    }

    #[test]
    fn build_mvt_sql_includes_core_fragments() {
        let sql = build_mvt_sql(
            &introspection_with_srid(4326),
            &GeoParquetEntry::default(),
            "buildings",
            &from_expr(),
        );

        insta::assert_snapshot!(sql, @r#"

        SELECT ST_AsMVT(tile, 'buildings', 4096, 'geom')
        FROM (
          SELECT
            ST_AsMVTGeom(
                ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true),
                ST_Extent(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER)),
                4096::BIGINT, 64::BIGINT, true
            ) AS geom
            , "category"::VARCHAR AS "category", "name"::VARCHAR AS "name"
          FROM read_parquet('/data/points.parquet')
          WHERE ST_Intersects(ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true), ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER)))
        ) AS tile;
        "#);
    }

    #[test]
    fn build_mvt_sql_expands_bounds_for_buffered_non_wgs84_sources() {
        let sql = build_mvt_sql(
            &introspection_with_srid(3857),
            &GeoParquetEntry::default(),
            "buildings",
            &from_expr(),
        );

        insta::assert_snapshot!(sql, @r#"

        SELECT ST_AsMVT(tile, 'buildings', 4096, 'geom')
        FROM (
          SELECT
            ST_AsMVTGeom(
                ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:3857'), 'EPSG:3857', 'EPSG:3857', always_xy := true),
                ST_Extent(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER)),
                4096::BIGINT, 64::BIGINT, true
            ) AS geom
            , "category"::VARCHAR AS "category", "name"::VARCHAR AS "name"
          FROM read_parquet('/data/points.parquet')
          WHERE ST_Intersects(ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:3857'), 'EPSG:3857', 'EPSG:3857', always_xy := true), ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER)))
        ) AS tile;
        "#);
    }

    #[test]
    fn build_mvt_sql_skips_bounds_expansion_when_buffer_is_zero() {
        let entry = GeoParquetEntry {
            buffer: Some(0),
            ..GeoParquetEntry::default()
        };
        let sql = build_mvt_sql(
            &introspection_with_srid(4326),
            &entry,
            "buildings",
            &from_expr(),
        );

        insta::assert_snapshot!(sql, @r#"

        SELECT ST_AsMVT(tile, 'buildings', 4096, 'geom')
        FROM (
          SELECT
            ST_AsMVTGeom(
                ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true),
                ST_Extent(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER)),
                4096::BIGINT, 0::BIGINT, true
            ) AS geom
            , "category"::VARCHAR AS "category", "name"::VARCHAR AS "name"
          FROM read_parquet('/data/points.parquet')
          WHERE ST_Intersects(ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true), ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER))
        ) AS tile;
        "#);
    }

    #[test]
    fn build_mvt_sql_compares_the_covering_against_the_tile_in_the_source_crs() {
        let sql = build_mvt_sql(
            &introspection_with_covering(4326),
            &GeoParquetEntry::default(),
            "buildings",
            &from_expr(),
        );

        insta::assert_snapshot!(sql, @r#"

        SELECT ST_AsMVT(tile, 'buildings', 4096, 'geom')
        FROM (
          SELECT
            ST_AsMVTGeom(
                ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true),
                ST_Extent(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER)),
                4096::BIGINT, 64::BIGINT, true
            ) AS geom
            , "category"::VARCHAR AS "category", "name"::VARCHAR AS "name"
          FROM read_parquet('/data/points.parquet')
          WHERE "bbox"."xmin" <= ST_XMax(ST_Extent(ST_Transform(ST_Intersection(ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER)), ST_TileEnvelope(0, 0, 0)), 'EPSG:3857', 'EPSG:4326', always_xy := true)))
            AND "bbox"."xmax" >= ST_XMin(ST_Extent(ST_Transform(ST_Intersection(ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER)), ST_TileEnvelope(0, 0, 0)), 'EPSG:3857', 'EPSG:4326', always_xy := true)))
            AND "bbox"."ymin" <= ST_YMax(ST_Extent(ST_Transform(ST_Intersection(ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER)), ST_TileEnvelope(0, 0, 0)), 'EPSG:3857', 'EPSG:4326', always_xy := true)))
            AND "bbox"."ymax" >= ST_YMin(ST_Extent(ST_Transform(ST_Intersection(ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER)), ST_TileEnvelope(0, 0, 0)), 'EPSG:3857', 'EPSG:4326', always_xy := true)))
            AND ST_Intersects(ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:4326'), 'EPSG:4326', 'EPSG:3857', always_xy := true), ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER)))
        ) AS tile;
        "#);
    }

    #[test]
    fn build_mvt_sql_needs_no_transform_when_the_source_is_already_web_mercator() {
        let sql = build_mvt_sql(
            &introspection_with_covering(3857),
            &GeoParquetEntry::default(),
            "buildings",
            &from_expr(),
        );

        insta::assert_snapshot!(sql, @r#"

        SELECT ST_AsMVT(tile, 'buildings', 4096, 'geom')
        FROM (
          SELECT
            ST_AsMVTGeom(
                ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:3857'), 'EPSG:3857', 'EPSG:3857', always_xy := true),
                ST_Extent(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER)),
                4096::BIGINT, 64::BIGINT, true
            ) AS geom
            , "category"::VARCHAR AS "category", "name"::VARCHAR AS "name"
          FROM read_parquet('/data/points.parquet')
          WHERE "bbox"."xmin" <= ST_XMax(ST_Extent(ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER))))
            AND "bbox"."xmax" >= ST_XMin(ST_Extent(ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER))))
            AND "bbox"."ymin" <= ST_YMax(ST_Extent(ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER))))
            AND "bbox"."ymax" >= ST_YMin(ST_Extent(ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER))))
            AND ST_Intersects(ST_Transform(ST_SetCRS("geom"::GEOMETRY, 'EPSG:3857'), 'EPSG:3857', 'EPSG:3857', always_xy := true), ST_Expand(ST_TileEnvelope($z::INTEGER, $x::INTEGER, $y::INTEGER), ((0.015625)::DOUBLE * (40075016.6855785)::DOUBLE) / power(2, $z::INTEGER)))
        ) AS tile;
        "#);
    }

    #[test]
    fn build_mvt_sql_leaves_out_the_covering_for_a_projected_source() {
        let sql = build_mvt_sql(
            &introspection_with_covering(25832),
            &GeoParquetEntry::default(),
            "buildings",
            &from_expr(),
        );

        assert!(
            !sql.contains("bbox"),
            "unexpected covering predicate: {sql}"
        );
    }
}
