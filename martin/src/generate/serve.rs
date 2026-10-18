//! Plans a table source with `layers` for the tile server, which renders its tiles with Martin's engine.

use std::num::NonZeroU32;
use std::sync::Arc;

use martin_core::tiles::postgres::{PostgresEngineInfo, ScanLayout};
use martin_tilegen::plan::Plan;
use martin_tilegen::{MAX_ZOOM, TileGenError};

use super::layers::{LowerOptions, lower_table};
use super::{GenerateError, GenerateResult};
use crate::config::args::OrderByType;
use crate::config::file::postgres::TableInfo;
use crate::config::file::postgres::resolver::engine_sql;
use crate::config::file::tiling::Prefetch;

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
/// high for the extent, `max_feature_count` meets a layer's `where` or `geometry`, or the table's query
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
    if max_feature_count.is_some()
        && let Some(layer) = table
            .def
            .layers
            .iter()
            .find(|layer| layer.filter.is_some() || layer.geometry.is_some())
    {
        return Err(GenerateError::LimitedLayerFilter(layer.name.clone()));
    }
    let margin = table
        .def
        .layers
        .iter()
        .map(|layer| f64::from(layer.grid.buffer) / f64::from(layer.grid.extent))
        .fold(0.0, f64::max);
    let plan = Plan::new(vec![table.def]).map_err(TileGenError::from)?;
    let sql = engine_sql(&table.info, plan.columns(0), max_feature_count, order_by)?;
    Ok(PostgresEngineInfo {
        layout: ScanLayout {
            table: 0,
            crs: sql.crs,
            has_id: sql.has_id,
            properties: sql.properties.len(),
        },
        sql_query: sql.sql,
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
    fn max_feature_count_rejects_a_layer_with_where() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
            layers:
              major:
                where: "class == 'motorway'"
        "#});
        let err = engine_info("roads", &mut info, Some(100), None).expect_err("cannot limit");
        insta::assert_snapshot!(err, @"layer `major`: `max_feature_count` with a layer's `where` or `geometry` is not supported yet");
    }

    #[test]
    fn max_feature_count_rejects_a_layer_with_geometry() {
        let mut info = table(indoc! {"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
            layers:
              lines:
                geometry: line
        "});
        let err = engine_info("roads", &mut info, Some(100), None).expect_err("cannot limit");
        insta::assert_snapshot!(err, @"layer `lines`: `max_feature_count` with a layer's `where` or `geometry` is not supported yet");
    }

    #[test]
    fn max_feature_count_limits_a_layer_without_where_or_geometry() {
        let mut info = table(indoc! {"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
            layers:
              all: {}
        "});
        let engine = engine_info("roads", &mut info, Some(100), None).expect("plans");
        insta::assert_snapshot!(engine.sql_query, @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class" FROM "public"."roads" WHERE "geom" IS NOT NULL AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857)) LIMIT 100"#);
    }

    #[test]
    fn without_max_feature_count_a_layer_with_where_is_served() {
        let mut info = table(indoc! {r#"
            schema: public
            table: roads
            srid: 3857
            geometry_column: geom
            properties:
              class: text
            layers:
              major:
                where: "class == 'motorway'"
        "#});
        let engine = engine_info("roads", &mut info, None, None).expect("plans");
        insta::assert_snapshot!(engine.sql_query, @r#"SELECT ST_AsBinary(ST_Force2D(ST_CurveToLine("geom"::geometry))), "class" FROM "public"."roads" WHERE "geom" IS NOT NULL AND ("geom" && ST_MakeEnvelope($1, $2, $3, $4, 3857) OR "geom" && ST_MakeEnvelope($5, $6, $7, $8, 3857))"#);
    }
}
