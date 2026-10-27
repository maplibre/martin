use crate::source::SourceError;
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

    #[error("source partition {0} does not fit the sort key")]
    PartitionOverflow(u32),

    #[error("source row {0} does not fit the sort key")]
    RowOverflow(u64),

    #[error("a {0}-byte record does not fit a sort buffer")]
    RecordTooLarge(usize),

    #[error("invalid sort configuration: {0}")]
    InvalidSortConfig(&'static str),

    #[error(transparent)]
    RenderLayer(#[from] crate::RenderLayerError),

    #[error("feature batch names table {0}, but the plan has fewer tables")]
    UnknownTable(u16),

    #[error(transparent)]
    Source(SourceError),

    #[error(transparent)]
    Geometry(#[from] crate::GeometryError),

    #[error(transparent)]
    Wkb(#[from] crate::source::WkbError),

    #[error("feature {id}: {error}")]
    FeatureWkb {
        table: u16,
        id: u64,
        #[source]
        error: crate::source::WkbError,
    },

    #[error("the feature in row {row} of source partition {partition}: {error}")]
    RowWkb {
        table: u16,
        partition: u32,
        row: u64,
        #[source]
        error: crate::source::WkbError,
    },

    #[error(transparent)]
    Plan(#[from] crate::plan::PlanError),

    #[error("a temp record is corrupt")]
    CorruptRecord,

    #[error(transparent)]
    Mlt(#[from] mlt_core::MltError),

    #[error(transparent)]
    Expr(#[from] crate::expr::ExprError),

    #[error("{0:?} compression cannot be used here")]
    UnsupportedEncoding(martin_tile_utils::Encoding),

    #[error("the tile writer stopped early")]
    WriterStopped,

    #[error("output {} already exists and is not empty", .0.display())]
    OutputNotEmpty(std::path::PathBuf),

    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[error("writing a temp file failed: {0}")]
    TempWrite(#[from] wincode::WriteError),

    #[error("reading a temp file failed: {0}")]
    TempRead(#[from] wincode::ReadError),

    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    Mbtiles(#[from] mbtiles::MbtError),

    #[error(transparent)]
    Pmtiles(#[from] pmtiles::PmtError),
}

impl From<SourceError> for TileGenError {
    fn from(err: SourceError) -> Self {
        if matches!(err, SourceError::Stopped) {
            Self::WriterStopped
        } else {
            Self::Source(err)
        }
    }
}
