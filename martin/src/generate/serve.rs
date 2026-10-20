//! Plans a table source with `layers` for the tile server, which renders its tiles with Martin's engine.

use std::num::NonZeroU32;
use std::sync::Arc;

use martin_core::tiles::postgres::{PostgresEngineInfo, ScanLayout};
use martin_tilegen::plan::{Plan, RowFilter};
use martin_tilegen::{MAX_ZOOM, TileGenError};

use super::{GenerateError, GenerateResult};
use crate::config::args::OrderByType;
use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::engine_sql;
use crate::config::file::tiling::Prefetch;
use crate::generate::layers::{LowerOptions, lower_table};

const DEFAULT_EXTENT: u32 = 4096;
/// The engine quantizes each zoom into one `i32` grid of `extent << zoom` units, at most `2^30`.
const GRID_LIMIT: u64 = 1 << 30;

/// The plan and query the tiles of `info` are rendered from, reading at most `max_feature_count`
/// features per tile, chosen by `order_by`. Without a `maxzoom`, the table gets the highest zoom its
/// extent fits in the engine's grid, written back to `info`.
///
/// # Errors
///
/// When a layer uses a setting the engine does not support yet, no layer has a zoom, a zoom is too
/// high for the extent, `max_feature_count` meets a `where` SQL cannot apply, or the table's query
/// cannot be built.
pub fn engine_info(
    id: &str,
    info: &mut TableInfo,
    max_feature_count: Option<usize>,
    order_by: Option<OrderByType>,
) -> GenerateResult<PostgresEngineInfo> {
    let extent = info.extent.map_or(DEFAULT_EXTENT, NonZeroU32::get);
    let max_zoom = *info.maxzoom.get_or_insert_with(|| max_grid_zoom(extent));
    let options = LowerOptions {
        zooms: 0..=MAX_ZOOM,
        bounds: None,
    };
    let table = lower_table(id, info.clone(), &options)?
        .ok_or(GenerateError::NoLayers(info.minzoom.unwrap_or(0), max_zoom))?;
    let margin = table
        .def
        .layers
        .iter()
        .map(|layer| (f64::from(layer.grid.buffer) + 0.5) / f64::from(layer.grid.extent))
        .fold(0.0, f64::max);
    let plan = Plan::new(vec![table.def]).map_err(TileGenError::from)?;
    let mut sql = (0..=max_zoom)
        .map(|zoom| {
            let rows = plan.row_filter(0, zoom..=zoom);
            if let RowFilter::Untranslatable(err) = &rows
                && max_feature_count.is_some()
            {
                return Err(GenerateError::UnlimitableWhere(err.clone()));
            }
            let sql = engine_sql(
                &table.info,
                plan.columns(0),
                rows.condition(),
                max_feature_count,
                order_by,
            )?;
            Ok(sql)
        })
        .collect::<GenerateResult<Vec<_>>>()?;
    let last = sql.pop().expect("zoom 0 has a query");
    Ok(PostgresEngineInfo {
        layout: ScanLayout {
            table: 0,
            crs: last.crs,
            has_id: last.has_id,
            properties: last.properties.len(),
        },
        sql_queries: sql
            .into_iter()
            .map(|sql| sql.sql)
            .chain([last.sql])
            .collect(),
        plan: Arc::new(plan),
        margin,
        prefetch: info
            .prefetch
            .map(Prefetch::tiles_per_side)
            .filter(|side| side.get() > 1),
    })
}

/// The highest zoom whose grid of `extent` units per tile fits the engine's.
fn max_grid_zoom(extent: u32) -> u8 {
    (0..=MAX_ZOOM)
        .rev()
        .find(|&zoom| u64::from(extent) << zoom <= GRID_LIMIT)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use super::*;

    fn table(yaml: &str) -> TableInfo {
        serde_saphyr::from_str(yaml).expect("parses")
    }

    #[test]
    fn the_engine_query_keeps_the_rows_of_each_layers_geometry() {
        let mut info = table(indoc! {"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
            layers:
              stops:
                geometry: point
              paths:
                geometry: line
        "});
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[0], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class" FROM "public"."roads" WHERE "geom" IS NOT NULL AND (ST_GeometryType(ST_CurveToLine("geom"::geometry)) IN ('ST_Point', 'ST_MultiPoint') OR ST_GeometryType(ST_CurveToLine("geom"::geometry)) IN ('ST_LineString', 'ST_MultiLineString')) AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn the_engine_query_below_a_layers_minzoom_skips_its_rows() {
        let mut info = table(indoc! {"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            maxzoom: 1
            properties:
              class: text
            layers:
              detail:
                minzoom: 1
                geometry: line
              overview:
                geometry: polygon
        "});
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[0], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class" FROM "public"."roads" WHERE "geom" IS NOT NULL AND (ST_GeometryType(ST_CurveToLine("geom"::geometry)) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn the_engine_query_at_a_layers_minzoom_keeps_its_rows() {
        let mut info = table(indoc! {"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            maxzoom: 1
            properties:
              class: text
            layers:
              detail:
                minzoom: 1
                geometry: line
              overview:
                geometry: polygon
        "});
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[1], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class" FROM "public"."roads" WHERE "geom" IS NOT NULL AND (ST_GeometryType(ST_CurveToLine("geom"::geometry)) IN ('ST_LineString', 'ST_MultiLineString') OR ST_GeometryType(ST_CurveToLine("geom"::geometry)) IN ('ST_Polygon', 'ST_MultiPolygon', 'ST_Triangle')) AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn the_engine_query_with_a_layer_of_any_geometry_reads_every_row() {
        let mut info = table(indoc! {"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
            layers:
              paths:
                geometry: line
              all: {}
        "});
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[0], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class" FROM "public"."roads" WHERE "geom" IS NOT NULL AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn the_engine_query_keeps_the_rows_some_layer_takes() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
              rank: int4
            layers:
              major:
                where: "class == 'motorway' && rank > 2"
              paths:
                geometry: line
                where: "rank < 1"
        "#});
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[0], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class", "rank" FROM "public"."roads" WHERE "geom" IS NOT NULL AND (("class" = 'motorway' AND "rank" > 2) OR (ST_GeometryType(ST_CurveToLine("geom"::geometry)) IN ('ST_LineString', 'ST_MultiLineString') AND "rank" < 1)) AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn max_feature_count_rejects_a_where_sql_cannot_apply() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              name: text
            layers:
              named:
                where: "name + '!' == 'A!'"
        "#});
        let err = engine_info("roads", &mut info, Some(100), None).expect_err("cannot limit");
        insta::assert_snapshot!(err, @"layer `named`: `max_feature_count` needs `where` checked in SQL, but `name + '!' == 'A!'` uses `+`, which has no SQL equivalent");
    }

    #[test]
    fn max_feature_count_rejects_a_where_sql_cannot_apply_beside_a_layer_of_any_geometry() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              name: text
            layers:
              all: {}
              named:
                where: "name + '!' == 'A!'"
        "#});
        let err = engine_info("roads", &mut info, Some(100), None).expect_err("cannot limit");
        insta::assert_snapshot!(err, @"layer `named`: `max_feature_count` needs `where` checked in SQL, but `name + '!' == 'A!'` uses `+`, which has no SQL equivalent");
    }

    #[test]
    fn the_engine_query_compares_a_nondeterministic_column_by_code_point() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
              name: text
            layers:
              major:
                where: "class == 'motorway' && name != 'A1'"
        "#});
        info.discovered
            .nondeterministic_columns
            .insert("name".to_owned());
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[0], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class", "name" FROM "public"."roads" WHERE "geom" IS NOT NULL AND ("class" = 'motorway' AND "name" COLLATE "C" IS DISTINCT FROM 'A1') AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn the_engine_query_compares_two_string_columns_by_code_point() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              name: text
              name_en: text
            layers:
              translated:
                where: "name != name_en"
        "#});
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[0], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "name", "name_en" FROM "public"."roads" WHERE "geom" IS NOT NULL AND ("name" COLLATE "C" IS DISTINCT FROM "name_en") AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn attributes_without_jsonb_keys_let_the_engine_query_apply_where() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
              tags: jsonb
            layers:
              major:
                attributes: [class]
                where: "class == 'motorway'"
        "#});
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[0], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class" FROM "public"."roads" WHERE "geom" IS NOT NULL AND ("class" = 'motorway') AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn max_feature_count_rejects_a_where_on_a_name_column() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              ref: name
            layers:
              numbered:
                where: "ref == 'A1'"
        "#});
        let err = engine_info("roads", &mut info, Some(100), None).expect_err("cannot limit");
        insta::assert_snapshot!(err, @"layer `numbered`: `max_feature_count` needs `where` checked in SQL, but `ref == 'A1'` compares `ref`, whose type the source cannot compare");
    }

    #[test]
    fn max_feature_count_rejects_a_where_on_a_bpchar_column() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              code: bpchar
            layers:
              coded:
                where: "code != 'A'"
        "#});
        let err = engine_info("roads", &mut info, Some(100), None).expect_err("cannot limit");
        insta::assert_snapshot!(err, @"layer `coded`: `max_feature_count` needs `where` checked in SQL, but `code != 'A'` compares `code`, whose type the source cannot compare");
    }

    #[test]
    fn max_feature_count_rejects_a_where_on_a_domain_over_bpchar() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              code: road_code
            layers:
              coded:
                where: "code != 'A'"
        "#});
        info.discovered
            .column_types
            .insert("code".to_owned(), "bpchar".to_owned());
        let err = engine_info("roads", &mut info, Some(100), None).expect_err("cannot limit");
        insta::assert_snapshot!(err, @"layer `coded`: `max_feature_count` needs `where` checked in SQL, but `code != 'A'` compares `code`, whose type the source cannot compare");
    }

    #[test]
    fn without_max_feature_count_a_where_sql_cannot_apply_reads_every_row() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              name: text
            layers:
              named:
                where: "name + '!' == 'A!'"
        "#});
        let engine = engine_info("roads", &mut info, None, None).expect("plans");
        insta::assert_snapshot!(engine.sql_queries[0], @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "name" FROM "public"."roads" WHERE "geom" IS NOT NULL AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857))"#);
    }

    #[test]
    fn margin_reads_half_a_grid_unit_past_the_buffer() {
        let mut info = table(indoc! {"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            extent: 4096
            buffer: 64
            properties:
              class: text
            layers:
              all: {}
        "});
        let engine = engine_info("roads", &mut info, None, None).expect("plans");
        assert_eq!(engine.margin * 4096.0, 64.5);
    }
}
