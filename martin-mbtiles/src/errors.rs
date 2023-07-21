use std::path::PathBuf;

use crate::mbtiles::MbtType;
use martin_tile_utils::TileInfo;

#[derive(thiserror::Error, Debug)]
pub enum MbtError {
    #[error("SQL Error {0}")]
    SqlError(#[from] sqlx::Error),

    #[error("MBTile filepath contains unsupported characters: {}", .0.display())]
    UnsupportedCharsInFilepath(PathBuf),

    #[error("Inconsistent tile formats detected: {0} vs {1}")]
    InconsistentMetadata(TileInfo, TileInfo),

    #[error("Invalid data format for MBTile file {0}")]
    InvalidDataFormat(String),

    #[error("Incorrect data format for MBTile file {0}; expected {1:?} and got {2:?}")]
    IncorrectDataFormat(String, MbtType, MbtType),

    #[error(r#"Filename "{0}" passed to SQLite must be valid UTF-8"#)]
    InvalidFilenameType(PathBuf),

    #[error("No tiles found")]
    NoTilesFound,

    #[error("The destination file {0} is non-empty")]
    NonEmptyTargetFile(PathBuf),

    #[error("The file {0} is does not have the required uniqueness constraint")]
    NoUniquenessConstraint(String),

    #[error("Unexpected duplicate values found when copying")]
    DuplicateValues(),
}

pub type MbtResult<T> = Result<T, MbtError>;
