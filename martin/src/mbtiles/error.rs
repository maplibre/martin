#[derive(thiserror::Error, Debug)]
pub enum MbtilesError {
    #[error(r"Unable to acquire connection to file: {0}")]
    AcquireConnError(String),

    #[error(transparent)]
    MbtilesLibraryError(#[from] mbtiles::MbtError),
}
