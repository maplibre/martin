use crate::source::Xyz;

pub type Result<T> = std::result::Result<T, PmtError>;

#[derive(thiserror::Error, Debug)]
pub enum PmtError {
    #[error("IO Error {0}")]
    IoError(#[from] std::io::Error),

    #[error("Source path is not a file: {}", .0.display())]
    InvalidFilePath(std::path::PathBuf),

    #[error("Source {0} uses bad file {}", .1.display())]
    InvalidSourceFilePath(String, std::path::PathBuf),

    #[error(r#"Tile {0:#} not found in {1}"#)]
    GetTileError(Xyz, String),
}
