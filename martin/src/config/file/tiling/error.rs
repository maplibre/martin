#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TilingConfigError {
    #[error("`layers` needs at least one layer")]
    NoLayers,
    #[error("layer `{0}`: {1}")]
    InLayer(String, Box<Self>),
    #[error(
        "table `{0}`: `layer_id` names the layer PostGIS tiles; with `layers`, each layer is named by its key, so drop `layer_id`"
    )]
    LayerIdWithLayers(String),

    #[error("minzoom {min} is above maxzoom {max}")]
    MinzoomAboveMaxzoom { min: u8, max: u8 },
    #[error("a zoom cannot change with the zoom")]
    ZoomStepsForZoom,
    #[error("zoom steps cannot be mixed with `{0}`")]
    ZoomStepsMixedWith(String),
    #[error("needs zoom steps such as `{{ 0: 2, 11: 0 }}`, or one of `match`, `lookup` or `expr`")]
    NoSetting,
    #[error("pick one of `match`, `lookup` or `expr`")]
    SeveralSettings,

    #[error("`let.` needs the name of a computed value after it")]
    EmptyLetName,
    #[error("a property name cannot be empty")]
    EmptyPropertyName,
    #[error("`*` needs a prefix before it, such as `name:*`")]
    WildcardWithoutPrefix,
    #[error("`struct` needs at least one field")]
    EmptyStruct,
    #[error("`{0}` only applies to a whole attribute")]
    WholeAttributeOnly(&'static str),
    #[error("`desc` only applies to a key of `sort_by`")]
    DescOutsideSortBy,
    #[error("needs one of `value`, `from`, `coalesce`, `struct`, `match`, `lookup` or `expr`")]
    NoValue,
    #[error("pick one of `{}`", .0.join("`, `"))]
    SeveralValues(Vec<&'static str>),
    #[error("`map` belongs to `lookup`")]
    MapWithoutLookup,
    #[error("`lookup` needs a `map`")]
    LookupWithoutMap,
    #[error("`map` needs at least one entry")]
    EmptyLookupMap,
    #[error("`else` goes with `lookup`; in a `match`, write it as its last case")]
    ElseWithoutLookup,
    #[error("`else` does not apply to `{0}`; it goes with `from` or `lookup`")]
    ElseNotApplicable(&'static str),
    #[error("`else` must be the last case")]
    ElseNotLast,
    #[error("a case with `if` needs a `value`")]
    CaseWithoutValue,
    #[error("a case is either `{{ if: condition, value: … }}` or `{{ else: … }}`")]
    MalformedCase,
    #[error("`match` needs at least one `if` case")]
    NoIfCase,
    #[error("a wildcard column copies the properties it names: write `\"{0}\": \"{0}\"`")]
    RenamedWildcard(String),
    #[error("`{column}` is one column, so it cannot copy every `{property}` property")]
    WildcardIntoOneColumn { column: String, property: String },

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

    #[error("a rule without `where` takes every feature, so it must be the last rule")]
    CatchAllNotLast,
    #[error("`rules` needs at least one rule with a `where`")]
    NoConditionalRule,

    #[error("pick one of `min_length` (pixels) or `min_length_m` (metres)")]
    PixelAndMetreLength,
    #[error("`label_grid` needs a `limit`, a `rank_attribute`, or both")]
    LabelGridWithoutKeep,
}
