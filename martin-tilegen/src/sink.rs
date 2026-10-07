use martin_tile_utils::TileCoord;
use tilejson::TileJSON;

use crate::{TileGenResult, TileOrder};

/// Whether a tile's content is expected to repeat elsewhere in the tileset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DedupHint {
    /// The content is not expected to repeat, so a sink may skip looking for earlier copies.
    Unique,
    /// The content is likely to repeat, so a sink should store it once.
    LikelyDuplicate,
}

/// A finished, compressed tile ready for a [`TileSink`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedTile {
    pub coord: TileCoord,
    pub data: Vec<u8>,
    pub hint: DedupHint,
}

/// A synchronous archive writer fed with tiles in ascending [`Self::tile_order`].
pub trait TileSink: Send {
    fn tile_order(&self) -> TileOrder;

    fn write(&mut self, batch: &[EncodedTile]) -> TileGenResult<()>;

    fn finish(self, meta: &TileJSON) -> TileGenResult<()>;
}
