use martin_tile_utils::TileCoord;

use crate::tile::MAX_ZOOM;

pub type TileGenResult<T> = Result<T, TileGenError>;

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum TileGenError {
    #[error("tile {0:#} does not exist or is above zoom {MAX_ZOOM}")]
    InvalidTile(TileCoord),

    #[error("tile id {0} is above the highest tile id of zoom {MAX_ZOOM}")]
    InvalidTileId(u64),

    #[error("source sequence (partition {partition}, row {row}) does not fit the sort key")]
    SeqOverflow { partition: u32, row: u64 },

    #[error("a {0}-byte record does not fit a sort buffer")]
    RecordTooLarge(usize),

    #[error("invalid sort configuration: {0}")]
    InvalidSortConfig(&'static str),

    #[error("temp file I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Pmtiles(#[from] pmtiles::PmtError),
}
