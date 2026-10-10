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

    #[error("layer {index}: extent {extent} at max zoom {max_zoom} overflows the i32 grid")]
    ZoomGridOverflow {
        index: u8,
        extent: u32,
        max_zoom: u8,
    },

    #[error("layer {0} has no zooms")]
    EmptyZooms(u8),

    #[error("layer {index}: buffer {buffer} is not below half the extent {extent} or above 65535")]
    InvalidBuffer { index: u8, buffer: u32, extent: u32 },

    #[error("feature batch names table {0}, but the plan has fewer tables")]
    UnknownTable(u16),

    #[error("partition {0} does not exist")]
    UnknownPartition(u32),

    #[error("a line with {0} vertices does not fit a record")]
    TooManyVertices(usize),

    #[error("a coordinate is outside the i32 zoom grid")]
    CoordOverflow,

    #[error(transparent)]
    Slice(#[from] map_tile_toolkit::TileError),

    #[error("{0} layers exceed the 256 a sort key can address")]
    TooManyLayers(usize),

    #[error("{0} tables exceed the 65536 a feature batch can address")]
    TooManyTables(usize),

    #[error("layer name `{0}` is used more than once")]
    DuplicateLayer(String),

    #[error("layer `{layer}`: zoom {zoom} is above {MAX_ZOOM}")]
    ZoomTooHigh { layer: String, zoom: u8 },

    #[error("layer `{layer}`: attribute `{key}` is listed more than once")]
    DuplicateAttribute { layer: String, key: String },

    #[error("layer `{layer}`: attribute `{column}` is not a table column")]
    UnknownColumn { layer: String, column: String },

    #[error("reading the source failed: {0}")]
    Source(Box<dyn std::error::Error + Send + Sync>),

    #[error("a temp record is corrupt")]
    CorruptRecord,

    #[error(transparent)]
    Mlt(#[from] mlt_core::MltError),

    #[error("{0:?} compression cannot be used here")]
    UnsupportedEncoding(martin_tile_utils::Encoding),

    #[error("the tile writer stopped early")]
    WriterStopped,

    #[error("output {} already exists and is not empty", .0.display())]
    OutputNotEmpty(std::path::PathBuf),

    #[error("I/O failed: {0}")]
    Io(#[from] std::io::Error),

    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    Mbtiles(#[from] mbtiles::MbtError),

    #[error(transparent)]
    Pmtiles(#[from] pmtiles::PmtError),
}
