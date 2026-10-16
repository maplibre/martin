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
/// high for the extent, or the table's query cannot be built.
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
        prefetch: info.prefetch.map_or(1, Prefetch::tiles_per_side),
    })
}

/// The highest zoom whose grid of `extent` units per tile fits the engine's.
fn max_grid_zoom(extent: u32) -> u8 {
    (0..=MAX_ZOOM)
        .rev()
        .find(|&zoom| u64::from(extent) << zoom <= GRID_LIMIT)
        .unwrap_or(0)
}
