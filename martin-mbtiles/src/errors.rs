use std::path::PathBuf;

use martin_tile_utils::TileInfo;
use sqlite_hashes::rusqlite;

#[derive(thiserror::Error, Debug)]
pub enum MbtError {
    #[error("SQL Error {0}")]
    SqlxError(#[from] sqlx::Error),

    #[error("SQL Error {0}")]
    RusqliteError(#[from] rusqlite::Error),

    #[error("MBTile filepath contains unsupported characters: {}", .0.display())]
    UnsupportedCharsInFilepath(PathBuf),

    #[error("Inconsistent tile formats detected: {0} vs {1}")]
    InconsistentMetadata(TileInfo, TileInfo),

    #[error("Invalid data format for MBTile file {0}")]
    InvalidDataFormat(String),

    #[error("Integrity check failed for MBTile file {0}")]
    FailedIntegrityCheck(String),

    #[error("Invalid tile data for MBTile file {0}")]
    InvalidTileData(String),

    #[error(r#"Filename "{0}" passed to SQLite must be valid UTF-8"#)]
    InvalidFilenameType(PathBuf),

    #[error("No tiles found")]
    NoTilesFound,

    #[error("The destination file {0} is non-empty")]
    NonEmptyTargetFile(PathBuf),

    #[error("The file {0} does not have the required uniqueness constraint")]
    NoUniquenessConstraint(String),

    #[error("Could not copy MBTiles file: {reason}")]
    UnsupportedCopyOperation { reason: String },

    #[error("Unexpected duplicate tiles found when copying")]
    DuplicateValues,
}

pub type MbtResult<T> = Result<T, MbtError>;
