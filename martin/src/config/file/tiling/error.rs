#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TilingConfigError {
    #[error("`layers` needs at least one layer")]
    NoLayers,
    #[error("layer `{0}`: {1}")]
    InLayer(String, Box<Self>),

    #[error("minzoom {min} is above maxzoom {max}")]
    MinzoomAboveMaxzoom { min: u8, max: u8 },
}
