#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TilingConfigError {
    #[error("`layers` needs at least one layer")]
    NoLayers,
    #[error("layer `{0}`: {1}")]
    InLayer(String, MinzoomAboveMaxzoom),
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("`{expr}` is not a valid CEL expression: {reason}")]
pub struct InvalidExpr {
    pub expr: String,
    pub reason: String,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("minzoom {min} is above maxzoom {max}")]
pub struct MinzoomAboveMaxzoom {
    pub min: u8,
    pub max: u8,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
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
    #[error(transparent)]
    Zooms(#[from] MinzoomAboveMaxzoom),
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum RulesError {
    #[error("a rule without `where` takes every feature, so it must be the last rule")]
    CatchAllNotLast,
    #[error("`rules` needs at least one rule with a `where`")]
    NoConditionalRule,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TileOpError {
    #[error("pick one of `min_length` (pixels) or `min_length_m` (metres)")]
    PixelAndMetreLength,
    #[error("`label_grid` needs a `limit`, a `rank_attribute`, or both")]
    LabelGridWithoutKeep,
    #[error(transparent)]
    Zooms(#[from] MinzoomAboveMaxzoom),
}
