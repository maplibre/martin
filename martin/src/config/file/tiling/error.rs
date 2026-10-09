#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum TilingConfigError {
    #[error("`layers` needs at least one layer")]
    NoLayers,
    #[error("layer `{0}`: {1}")]
    InLayer(String, Box<Self>),

    #[error("minzoom {min} is above maxzoom {max}")]
    MinzoomAboveMaxzoom { min: u8, max: u8 },

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
}
