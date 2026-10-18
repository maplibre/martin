//! Serves a table with `layers`: one query reads the features around a tile, Martin's engine renders them.

use std::sync::Arc;

use martin_tile_utils::{EARTH_CIRCUMFERENCE, Encoding, TileCoord, TileData, TileRect};
use martin_tilegen::plan::Plan;
use martin_tilegen::{Summary, TileFormat, TileGenError, render_block};
use tracing::{debug, warn};

use super::scan::{ScanLayout, scan_features};
use super::{PostgresError, PostgresPool, PostgresResult};

/// How a table with `layers` is read and rendered.
#[derive(Clone, Debug)]
pub struct PostgresEngineInfo {
    /// The plan of the table's layers, the table being its only one.
    pub plan: Arc<Plan>,
    /// SQL taking `$1..$4` and `$5..$8` as two Web Mercator `xmin, ymin, xmax, ymax` envelopes of the
    /// area to read, and returning its features once each, as `layout` says.
    pub sql_query: String,
    /// How the query's columns map to the plan's table.
    pub layout: ScanLayout,
    /// How far past a tile its features are read, as a fraction of a tile's side: the largest buffer
    /// of the layers.
    pub margin: f64,
}

impl PostgresEngineInfo {
    /// Reads the features reaching `xyz` and renders them into an uncompressed MVT tile.
    pub(super) async fn render_tile(
        &self,
        pool: &PostgresPool,
        source_id: &str,
        xyz: TileCoord,
    ) -> PostgresResult<TileData> {
        let failed = |err: TileGenError| {
            PostgresError::EngineError(Box::new(err), source_id.to_owned(), xyz)
        };
        let block = TileRect::new(xyz.z(), xyz.x(), xyz.y(), xyz.x(), xyz.y());
        let [[x1, y1, x2, y2], [x3, y3, x4, y4]] = self.envelopes(block);
        let conn = pool.get().await?;
        let _cancel_guard = pool.active_query_registry().register(conn.cancel_token());
        let statement = conn
            .prepare_cached(&self.sql_query)
            .await
            .map_err(|e| PostgresError::PostgresError(e, "preparing a table's feature query"))?;
        debug!(sql = %self.sql_query, tile.coord = %xyz, "Running feature query");
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
            render_block(
                &plan,
                &mut batches,
                block,
                TileFormat::Mvt,
                Encoding::Uncompressed,
            )
        })
        .await
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic.into_panic()))
        .map_err(failed)?;
        report(source_id, xyz, &summary);
        Ok(tiles
            .into_iter()
            .next()
            .map(|tile| TileData::from(tile.data))
            .unwrap_or_default())
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
