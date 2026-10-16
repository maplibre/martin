//! Renders the tiles of one block in memory, for serving tiles as they are asked for.

use martin_tile_utils::{Encoding, TileRect};

use crate::phase1::RenderWorker;
use crate::phase2::{TileEncoder, TileEncoding};
use crate::plan::Plan;
use crate::sort::Sorter;
use crate::source::FeatureBatch;
use crate::{EncodedTile, Summary, TileFormat, TileGenResult, TileOrder};

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
    let mut worker = RenderWorker::new(plan, &sorter, order, Some(block));
    for batch in batches {
        worker.render_batch(batch)?;
    }
    let totals = worker.finish()?;

    let (encoding, batches) =
        TileEncoding::start(plan, sorter, format, encoding, order, BATCH_TILES)?;
    let mut encoder = TileEncoder::default();
    let mut tiles = Vec::new();
    for batch in batches {
        tiles.extend(encoding.encode(&mut encoder, batch?)?);
    }
    let summary = totals.into_summary(plan, tiles.len() as u64);
    Ok((tiles, summary))
}

/// Tiles grouped per encode call; a block rarely has more.
const BATCH_TILES: usize = 256;
