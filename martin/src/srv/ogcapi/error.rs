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

    /// No style has this id.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Style {style} does not exist")]
    UnknownStyle { style: String },

    /// A map parameter has a value that can't be used.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Parameter {parameter} has the invalid value {value}")]
    InvalidMapParameter { parameter: &'static str, value: String },

    /// Map parameters that can't be used together.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Parameters {parameters} can't be used together")]
    ConflictingMapParameters { parameters: &'static str },

    /// The map shows more than the renderer can draw at zoom 0 in `MAX_SIDE` pixels.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("The map area is too large, at most four times the world's width and height can be drawn")]
    MapAreaTooLarge,

    /// A CRS other than CRS84, EPSG:4326 and EPSG:3857 was given for a map parameter.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("CRS {crs} is not supported, only CRS84, EPSG:4326 and EPSG:3857 are")]
    UnsupportedCrs { crs: String },

    /// A map was asked for in a format other than PNG and JPEG.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Map format {format} is not supported, only png and jpeg are")]
    UnsupportedMapFormat { format: String },
}

impl martin_core::Classify for OgcApiError {
    fn kind(&self) -> martin_core::ErrorKind {
        match self {
            Self::UnknownTileMatrixSet(_) | Self::UnknownCollection(_) | Self::UnknownTileset { .. } => {
                martin_core::ErrorKind::NotFound
            }
            #[cfg(all(feature = "rendering", target_os = "linux"))]
            Self::UnknownStyle { .. } => martin_core::ErrorKind::NotFound,
            #[cfg(all(feature = "rendering", target_os = "linux"))]
            Self::InvalidMapParameter { .. }
            | Self::ConflictingMapParameters { .. }
            | Self::MapAreaTooLarge
            | Self::UnsupportedCrs { .. }
            | Self::UnsupportedMapFormat { .. } => martin_core::ErrorKind::InvalidInput,
        }
    }
}
