#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TilingConfigError {
    #[error("`layers` needs at least one layer")]
    NoLayers,
    #[error("layer `{layer}`: minzoom {min} is above maxzoom {max}")]
    LayerMinzoomAboveMaxzoom { layer: String, min: u8, max: u8 },
    #[error("`{expr}` is not a valid CEL expression: {reason}")]
    InvalidExpr { expr: String, reason: String },
    #[error("minzoom {min} is above maxzoom {max}")]
    MinzoomAboveMaxzoom { min: u8, max: u8 },
    #[error("a property name cannot be empty")]
    EmptyPropertyName,
    #[error("`*` needs a prefix before it, such as `name:*`")]
    WildcardWithoutPrefix,
    #[error("needs one of `value` or `expr`")]
    NoValue,
    #[error("pick one of `value` or `expr`")]
    SeveralValues,
    #[error("a wildcard column copies the properties it names: write `\"{0}\": \"{0}\"`")]
    RenamedWildcard(String),
    #[error("a rule without `where` takes every feature, so it must be the last rule")]
    CatchAllNotLast,
    #[error("`rules` needs at least one rule with a `where`")]
    NoConditionalRule,
    #[error("pick one of `min_length` (pixels) or `min_length_m` (metres)")]
    PixelAndMetreLength,
    #[error("`label_grid` needs a `limit`, a `rank_attribute`, or both")]
    LabelGridWithoutKeep,
}
