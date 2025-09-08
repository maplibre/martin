use pmtiles::PmtError;
use url::Url;

#[derive(thiserror::Error, Debug)]
pub enum PmtilesError {
    #[error(r"Error occurred in processing S3 source uri: {0}")]
    S3SourceError(String),

    #[error(transparent)]
    LibError(#[from] PmtError),

    #[error(r"PMTiles error {0:?} processing {1}")]
    LibErrorWithCtx(PmtError, String),

    #[error(r"Unable to parse metadata in file {1}: {0}")]
    InvalidUrlMetadata(String, Url),
}
