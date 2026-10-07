//! Tile archive writers, fed by the [pipeline](crate::pipeline) on its writer thread.

#[cfg(feature = "mbtiles")]
mod mbtiles;

use martin_tile_utils::TileCoord;
#[cfg(feature = "mbtiles")]
pub use mbtiles::MbtilesSink;
use tilejson::TileJSON;

use crate::{TileGenResult, TileOrder};

/// One finished tile: encoded, compressed, and tagged for deduplication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedTile {
    pub coord: TileCoord,
    pub data: Vec<u8>,
    /// Tiles with the same key have identical bytes (the engine verified it), so a sink may store them once.
    /// `None` marks tiles the engine expects to be unique.
    pub dedup: Option<u64>,
}

pub trait TileSink: Send {
    /// The order the sink wants tiles in; the engine sorts by it.
    fn tile_order(&self) -> TileOrder;

    /// Writes batches of tiles in [`tile_order`](Self::tile_order). An `Err` batch means the generation
    /// failed upstream: the sink must stop and discard what it has not committed.
    fn write_all(
        &mut self,
        batches: &mut dyn Iterator<Item = TileGenResult<Vec<EncodedTile>>>,
    ) -> TileGenResult<()>;

    /// Stores the metadata, which is complete only after the last tile.
    fn finish(self, metadata: &TileJSON) -> TileGenResult<()>;
}
