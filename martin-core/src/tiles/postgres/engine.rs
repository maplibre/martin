//! Serves a table with `layers`: one query reads the features around a tile, Martin's engine renders them.

use std::collections::HashMap;
use std::num::NonZeroU8;
use std::sync::Arc;

use martin_tile_utils::{EARTH_CIRCUMFERENCE, Encoding, TileCoord, TileData, TileRect};
use martin_tilegen::plan::Plan;
use martin_tilegen::{Summary, TileFormat, TileGenError, render_block};
use tracing::{debug, warn};

use super::{PostgresError, PostgresPool, PostgresResult};
use crate::tiles::postgres::scan::{ScanLayout, scan_features};

/// How a table with `layers` is read and rendered.
#[derive(Clone, Debug)]
pub struct PostgresEngineInfo {
    /// The plan of the table's layers, the table being its only one.
    pub plan: Arc<Plan>,
    /// SQL by zoom from 0, the last one serving every zoom past it, taking `$1..$4` and `$5..$8` as two
    /// Web Mercator `xmin, ymin, xmax, ymax` envelopes of the area to read, and returning its features
    /// once each, as `layout` says.
    pub sql_queries: Vec<String>,
    /// How the query's columns map to the plan's table.
    pub layout: ScanLayout,
    /// How far past a tile its features are read, as a fraction of a tile's side: the largest buffer
    /// of the layers plus half a grid unit, since a vertex that far out still rounds onto the buffer edge.
    pub margin: f64,
    /// Tiles per side of the block a cache miss renders; `None` renders just the tile.
    pub prefetch: Option<NonZeroU8>,
}

impl PostgresEngineInfo {
    /// Reads the features reaching `xyz` and renders them into an uncompressed MVT tile.
    pub(super) async fn render_tile(
        &self,
        pool: &PostgresPool,
        source_id: &str,
        xyz: TileCoord,
    ) -> PostgresResult<TileData> {
        let block = TileRect::new(xyz.z(), xyz.x(), xyz.y(), xyz.x(), xyz.y());
        let tiles = self.render(pool, source_id, xyz, block).await?;
        Ok(tiles.into_iter().next().map(|(_, data)| data).unwrap_or_default())
    }

    /// Every tile of the [`prefetch`](Self::prefetch) block holding `xyz`, empty ones included, or
    /// `None` without prefetching. Blocks are aligned, so the tiles of one block share it.
    pub(super) async fn render_prefetch(
        &self,
        pool: &PostgresPool,
        source_id: &str,
        xyz: TileCoord,
    ) -> PostgresResult<Option<Vec<(TileCoord, TileData)>>> {
        let Some(side) = self.prefetch else {
            return Ok(None);
        };
        let side = u32::from(side.get());
        let last = (1u32 << xyz.z()) - 1;
        let (x, y) = (xyz.x() / side * side, xyz.y() / side * side);
        let block = TileRect::new(xyz.z(), x, y, (x + side - 1).min(last), (y + side - 1).min(last));
        let mut rendered: HashMap<_, _> = self.render(pool, source_id, xyz, block).await?.into_iter().collect();
        let tiles = (block.min_x..=block.max_x)
            .flat_map(|x| (block.min_y..=block.max_y).map(move |y| (x, y)))
            .map(|(x, y)| {
                let coord = TileCoord::new_unchecked(xyz.z(), x, y);
                (coord, rendered.remove(&coord).unwrap_or_default())
            })
            .collect();
        Ok(Some(tiles))
    }

    /// Reads the features reaching `block` and renders its tiles that have any, in TMS order.
    async fn render(
        &self,
        pool: &PostgresPool,
        source_id: &str,
        xyz: TileCoord,
        block: TileRect,
    ) -> PostgresResult<Vec<(TileCoord, TileData)>> {
        let failed = |err: TileGenError| PostgresError::EngineError(Box::new(err), source_id.to_owned(), xyz);
        let [[x1, y1, x2, y2], [x3, y3, x4, y4]] = self.envelopes(block);
        let sql = self.sql_query(block.zoom);
        let conn = pool.get().await?;
        let _cancel_guard = pool.active_query_registry().register(conn.cancel_token());
        let statement = conn
            .prepare_cached(sql)
            .await
            .map_err(|e| PostgresError::PostgresError(e, "preparing a table's feature query"))?;
        debug!(sql = %sql, tile.coord = %xyz, "Running feature query");
        let mut batches = Vec::new();
        scan_features(
            &conn,
            &statement,
            &[&x1, &y1, &x2, &y2, &x3, &y3, &x4, &y4],
            &self.layout,
            0,
            &self.plan.table_keys()[usize::from(self.layout.table)],
            |batch| {
                batches.push(batch);
                Ok(())
            },
        )
        .await
        .map_err(|err| failed(err.into()))?;
        drop(conn);

        let plan = Arc::clone(&self.plan);
        let (tiles, summary) = tokio::task::spawn_blocking(move || {
            render_block(&plan, &mut batches, block, TileFormat::Mvt, Encoding::Uncompressed)
        })
        .await
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic.into_panic()))
        .map_err(failed)?;
        report(source_id, xyz, &summary);
        Ok(tiles
            .into_iter()
            .map(|tile| (tile.coord, TileData::from(tile.data)))
            .collect())
    }

    /// The query reading the features of a block at `zoom`.
    fn sql_query(&self, zoom: u8) -> &str {
        self.sql_queries
            .get(usize::from(zoom))
            .or_else(|| self.sql_queries.last())
            .expect("an engine has a query for zoom 0")
    }

    /// The Web Mercator bounds of `block`, grown by [`Self::margin`] tiles on every side, as two
    /// envelopes within the world's columns: the second is the part that wraps past the antimeridian,
    /// or the first again when nothing does.
    fn envelopes(&self, block: TileRect) -> [[f64; 4]; 2] {
        let side = EARTH_CIRCUMFERENCE / f64::from(1u32 << block.zoom);
        let half = EARTH_CIRCUMFERENCE / 2.0;
        let margin = self.margin * side;
        let xmin = f64::from(block.min_x).mul_add(side, -half) - margin;
        let xmax = f64::from(block.max_x + 1).mul_add(side, -half) + margin;
        let ymin = half - f64::from(block.max_y + 1) * side - margin;
        let ymax = half - f64::from(block.min_y) * side + margin;
        let main = [xmin.max(-half), ymin, xmax.min(half), ymax];
        let wrapped = if xmax - xmin >= EARTH_CIRCUMFERENCE {
            main
        } else if xmin < -half {
            [xmin + EARTH_CIRCUMFERENCE, ymin, half, ymax]
        } else if xmax > half {
            [-half, ymin, xmax - EARTH_CIRCUMFERENCE, ymax]
        } else {
            main
        };
        [main, wrapped]
    }
}

/// Logs what rendering the block around `xyz` could not do. The plan already warns about the first
/// failure of each expression, so expression failures only show at debug level here.
fn report(source_id: &str, xyz: TileCoord, summary: &Summary) {
    for failed in &summary.slice_errors {
        warn!(
            source.id = source_id,
            tile.coord = %xyz,
            layer.id = %failed.layer,
            feature_zooms.skipped = failed.errors,
            feature.id = failed.id,
            tile.zoom = failed.zoom,
            error = %failed.error,
            "Skipped feature-zooms whose geometry the slicer could not handle"
        );
    }
    for failed in &summary.expr_errors {
        debug!(
            source.id = source_id,
            tile.coord = %xyz,
            "Layer `{}`: `{}` failed {} times and was taken as null",
            failed.layer,
            failed.expr,
            failed.errors
        );
    }
    debug!(
        source.id = source_id,
        tile.coord = %xyz,
        features = summary.features,
        features.skipped = summary.skipped,
        tiles = summary.tiles,
        "Rendered the block around a tile"
    );
}
