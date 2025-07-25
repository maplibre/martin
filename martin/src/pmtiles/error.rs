use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum PmtilesError {
    #[error(r"Error occurred in processing S3 source uri: {0}")]
    S3SourceError(String),

    #[cfg(feature = "pmtiles")]
    #[error(r"PMTiles error {0:?} processing {1}")]
    PmtilesLibraryError(pmtiles::PmtError, String),

    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidUrlMetadata(String, Url),

    #[error(r"Invalid tile coordinates")]
    InvalidTileCoordinates,
}
