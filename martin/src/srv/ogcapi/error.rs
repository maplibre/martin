/// Why an OGC API request failed.
#[derive(thiserror::Error, Debug)]
pub enum OgcApiError {
    /// No served source is on a tile matrix set with this id, or it has no OGC definition.
    #[error("Tile matrix set {0} does not exist")]
    UnknownTileMatrixSet(String),
}

impl martin_core::Classify for OgcApiError {
    fn kind(&self) -> martin_core::ErrorKind {
        match self {
            Self::UnknownTileMatrixSet(_) => martin_core::ErrorKind::NotFound,
        }
    }
}
