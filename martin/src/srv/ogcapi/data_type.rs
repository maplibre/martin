use martin_tile_utils::Format;
use serde::Serialize;

/// What a source's tiles hold, in the terms of OGC API - Tiles.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "unstable-schemas", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum DataType {
    Map,
    Vector,
}

impl DataType {
    /// Images are maps, everything else carries vector features.
    pub fn of(format: Format) -> Self {
        if Format::IMAGE_FORMATS.contains(&format) {
            Self::Map
        } else {
            Self::Vector
        }
    }

    /// The link relation from a collection to its tilesets of this type.
    pub const fn tilesets_rel(self) -> &'static str {
        match self {
            Self::Map => "http://www.opengis.net/def/rel/ogc/1.0/tilesets-map",
            Self::Vector => "http://www.opengis.net/def/rel/ogc/1.0/tilesets-vector",
        }
    }
}
