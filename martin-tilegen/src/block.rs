//! Renders the tiles of one block in memory, for serving tiles as they are asked for.

use martin_tile_utils::{Encoding, TileRect};

use crate::plan::Plan;
use crate::source::FeatureBatch;
use crate::tile_render::{FeatureRenderer, TileEncoding};
use crate::{
    EncodedTile, Sorter, Summary, TileEncoder, TileFormat, TileGenResult, TileGrouper, TileOrder,
};

/// Renders and encodes the tiles of `block` from `batches`, which must hold every feature reaching
/// those tiles or their buffer. The tiles come out as [`generate`](crate::generate) writes them, in
/// TMS order; a tile no feature reaches is left out. The summary counts what `batches` held and what
/// could not be rendered, as [`generate`](crate::generate) reports it.
///
/// # Errors
///
/// If a batch names a table the plan does not have, or a tile cannot be encoded.
pub fn render_block(
    plan: &Plan,
    batches: &mut [FeatureBatch],
    block: TileRect,
    format: TileFormat,
    encoding: Encoding,
) -> TileGenResult<(Vec<EncodedTile>, Summary)> {
    let order = TileOrder::Tms;
    let sorter = Sorter::in_memory();
    let mut buffer = sorter.buffer();
    let mut renderer = FeatureRenderer::new(plan, order, Some(block));
    for batch in batches {
        renderer.render_batch(batch, &mut buffer)?;
    }
    buffer.finish()?;
    let totals = renderer.finish();

    let encoding = TileEncoding::new(plan, format, encoding, order);
    let mut grouper = TileGrouper::new(sorter.merge()?);
    let mut encoder = TileEncoder::default();
    let mut tiles = Vec::new();
    loop {
        let batch = grouper.next_batch(BATCH_TILES)?;
        let done = batch.len() < BATCH_TILES;
        tiles.extend(encoding.encode(&mut encoder, batch)?);
        if done {
            break;
        }
    }
    let summary = totals.into_summary(plan, tiles.len() as u64);
    Ok((tiles, summary))
}

/// Tiles grouped per encode call; a block rarely has more.
const BATCH_TILES: usize = 256;
