use std::path::PathBuf;

use martin_tile_utils::TileInfo;

#[derive(thiserror::Error, Debug)]
pub enum MbtError {
    #[error("SQL Error {0}")]
    SqlError(#[from] sqlx::Error),

    #[error("MBTile filepath contains unsupported characters: {}", .0.display())]
    UnsupportedCharsInFilepath(PathBuf),

    #[error("Inconsistent tile formats detected: {0} vs {1}")]
    InconsistentMetadata(TileInfo, TileInfo),

    #[error("No tiles found")]
    NoTilesFound,
}

pub type MbtResult<T> = Result<T, MbtError>;
