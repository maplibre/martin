#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TilingConfigError {
    #[error("`layers` needs at least one layer")]
    NoLayers,
    #[error("layer `{0}`: {1}")]
    InLayer(String, Box<Self>),

    #[error("minzoom {min} is above maxzoom {max}")]
    MinzoomAboveMaxzoom { min: u8, max: u8 },

    #[error("property `{0}` is tested twice")]
    PropertyTestedTwice(String),
    #[error("the range is empty: no number lies between its ends")]
    EmptyRange,
    #[error("needs one of `like`, `gt`, `gte`, `lt` or `lte`")]
    NoPropertyTest,
    #[error("`like` cannot be combined with a range")]
    LikeWithRange,
    #[error("`{0}` repeats an end of the range that is already set")]
    RangeEndTwice(String),
}
