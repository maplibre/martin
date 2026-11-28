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
    #[cfg(feature = "rendering")]
    #[error("Style {style} does not exist")]
    UnknownStyle { style: String },

    /// A map parameter has a value that can't be used.
    #[cfg(feature = "rendering")]
    #[error("Parameter {parameter} has the invalid value {value}")]
    InvalidMapParameter { parameter: &'static str, value: String },

    /// Map parameters that can't be used together.
    #[cfg(feature = "rendering")]
    #[error("Parameters {parameters} can't be used together")]
    ConflictingMapParameters { parameters: &'static str },

    /// A CRS other than CRS84, EPSG:4326 and EPSG:3857 was given for a map parameter.
    #[cfg(feature = "rendering")]
    #[error("CRS {crs} is not supported, only CRS84, EPSG:4326 and EPSG:3857 are")]
    UnsupportedCrs { crs: String },

    /// The collection has no map, as its grid isn't web mercator or its tiles aren't images.
    #[cfg(feature = "rendering")]
    #[error("Collection {collection} has no map")]
    NoCollectionMap { collection: String },

    /// A `collections` parameter names something that isn't a collection a map can draw.
    #[cfg(feature = "rendering")]
    #[error("Collection {collection} does not exist or has no map")]
    UnknownMapCollection { collection: String },

    /// Map tiles were asked for on a grid other than `WebMercatorQuad`.
    #[cfg(feature = "rendering")]
    #[error("Maps are only tiled on WebMercatorQuad, not on {tile_matrix_set}")]
    UnknownMapTileset { tile_matrix_set: String },

    /// A map tile was asked for outside `WebMercatorQuad`.
    #[cfg(feature = "rendering")]
    #[error("Map tile {z}/{row}/{col} is outside WebMercatorQuad")]
    MapTileOutsideGrid { z: u8, row: u32, col: u32 },
}

impl martin_core::Classify for OgcApiError {
    fn kind(&self) -> martin_core::ErrorKind {
        match self {
            Self::UnknownTileMatrixSet(_) | Self::UnknownCollection(_) | Self::UnknownTileset { .. } => {
                martin_core::ErrorKind::NotFound
            }
            #[cfg(feature = "rendering")]
            Self::UnknownStyle { .. }
            | Self::NoCollectionMap { .. }
            | Self::UnknownMapTileset { .. }
            | Self::MapTileOutsideGrid { .. } => martin_core::ErrorKind::NotFound,
            #[cfg(feature = "rendering")]
            Self::InvalidMapParameter { .. }
            | Self::ConflictingMapParameters { .. }
            | Self::UnsupportedCrs { .. }
            | Self::UnknownMapCollection { .. } => martin_core::ErrorKind::InvalidInput,
        }
    }
}
