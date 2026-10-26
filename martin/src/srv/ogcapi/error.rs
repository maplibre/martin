/// Why an OGC API request failed.
#[derive(thiserror::Error, Debug)]
pub enum OgcApiError {
    /// No served source is on a tile matrix set with this id, or it has no OGC definition.
    #[error("Tile matrix set {0} does not exist")]
    UnknownTileMatrixSet(String),

    /// No served source has this id, or its tile matrix set has no OGC definition.
    #[error("Collection {0} does not exist")]
    UnknownCollection(String),

    /// The collection is served on another tile matrix set.
    #[error("Collection {collection} has no tileset on tile matrix set {tile_matrix_set}")]
    UnknownTileset {
        collection: String,
        tile_matrix_set: String,
    },
}

impl martin_core::Classify for OgcApiError {
    fn kind(&self) -> martin_core::ErrorKind {
        match self {
            Self::UnknownTileMatrixSet(_) | Self::UnknownCollection(_) | Self::UnknownTileset { .. } => {
                martin_core::ErrorKind::NotFound
            }
        }
    }
}
