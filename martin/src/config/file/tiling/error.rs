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
    #[error(
        "table `{0}`: `layer_id` names the layer PostGIS tiles; with `layers`, each layer is named by its key, so drop `layer_id`"
    )]
    LayerIdWithLayers(String),
    #[error("table `{0}`: `prefetch` only applies to a table with `layers`")]
    PrefetchWithoutLayers(String),
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
    #[error(
        "zooms {first_min}..={first_max} and {second_min}..={second_max} overlap; at each zoom at most one entry may run"
    )]
    OverlappingZooms {
        first_min: u8,
        first_max: u8,
        second_min: u8,
        second_max: u8,
    },
    #[error("`size` must be more than 0 pixels")]
    GridSizeNotPositive,
    #[error("table `{table}`: `{key}` is a layer key; move it under one of `layers`")]
    PerTileOnTable { table: String, key: &'static str },
    #[error("`{key}` acts on a whole tile, so it is set on the layer, not in a rule")]
    PerTileInRule { key: &'static str },
}
