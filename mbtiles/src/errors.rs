use std::path::PathBuf;

use martin_tile_utils::TileInfo;
use sqlite_hashes::rusqlite;

use crate::MbtType;

#[derive(thiserror::Error, Debug)]
pub enum MbtError {
    #[error("The source and destination MBTiles files are the same: {}", .0.display())]
    SameSourceAndDestination(PathBuf),

    #[error("The diff file and source or destination MBTiles files are the same: {}", .0.display())]
    SameDiffAndSourceOrDestination(PathBuf),

    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),

    #[error(transparent)]
    RusqliteError(#[from] rusqlite::Error),

    #[error(transparent)]
    JsonSerdeError(#[from] serde_json::Error),

    #[error("MBTile filepath contains unsupported characters: {}", .0.display())]
    UnsupportedCharsInFilepath(PathBuf),

    #[error("Inconsistent tile formats detected: {0} vs {1}")]
    InconsistentMetadata(TileInfo, TileInfo),

    #[error("Invalid data format for MBTile file {0}")]
    InvalidDataFormat(String),

    #[error("Integrity check failed for MBTile file {0} for the following reasons:\n    {1:?}")]
    FailedIntegrityCheck(String, Vec<String>),

    #[error("At least one tile has mismatching hash: stored value is `{1}` != computed value `{2}` in MBTile file {0}")]
    IncorrectTileHash(String, String, String),

    #[error("At least one tile in the tiles table/view has an invalid value: zoom_level={1}, tile_column={2}, tile_row={3} in MBTile file {0}")]
    InvalidTileIndex(String, String, String, String),

    #[error("Computed aggregate tiles hash {0} does not match tile data in metadata {1} for MBTile file {2}")]
    AggHashMismatch(String, String, String),

    #[error(
        "Metadata value `agg_tiles_hash` is not set in MBTiles file {0}\n    Use `mbtiles validate --agg-hash update {0}` to fix this."
    )]
    AggHashValueNotFound(String),

    #[error(r#"Filename "{0}" passed to SQLite must be valid UTF-8"#)]
    InvalidFilenameType(PathBuf),

    #[error("No tiles found")]
    NoTilesFound,

    #[error("The destination file {0} is not empty. Some operations like creating a diff file require the destination file to be non-existent or empty.")]
    NonEmptyTargetFile(PathBuf),

    #[error("The file {0} does not have the required uniqueness constraint")]
    NoUniquenessConstraint(String),

    #[error("Could not copy MBTiles file: {reason}")]
    UnsupportedCopyOperation { reason: String },

    #[error("Unexpected duplicate tiles found when copying")]
    DuplicateValues,

    #[error("Applying a patch while diffing is not supported")]
    CannotApplyPatchAndDiff,

    #[error("The MBTiles file {0} has data of type {1}, but the desired type was set to {2}")]
    MismatchedTargetType(PathBuf, MbtType, MbtType),
}

pub type MbtResult<T> = Result<T, MbtError>;
