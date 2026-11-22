use crate::Expr;
use thiserror::Error;

/// Crate-specific error enum.
///
/// Variants are added as new failure modes are found. Matching exhaustively on this enum would
/// make every such addition a breaking change, so it is marked non-exhaustive.
#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)]
#[non_exhaustive]
pub enum Error {
    /// [geojson::Error]
    #[error(transparent)]
    GeoJSON(#[from] geojson::Error),

    /// [geozero::error::GeozeroError]
    #[error(transparent)]
    Geozero(#[from] geozero::error::GeozeroError),

    /// Invalid CQL2 text
    #[error("invalid cql2-text: {0}")]
    InvalidCql2Text(String),

    /// Invalid number of arguments for the expression
    #[error("invalid number of arguments for {name}: {actual} (expected {expected})")]
    InvalidNumberOfArguments {
        /// The name of the expression or operation
        name: String,

        /// The actual number of arguments
        actual: usize,

        /// The number of arguments the expression or operation expected
        expected: usize,
    },

    /// [std::io::Error]
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Missing argument from a function that requires one.
    #[error("function {0} is missing a required argument")]
    MissingArgument(&'static str),

    /// [std::str::ParseBoolError]
    #[error(transparent)]
    ParseBool(#[from] std::str::ParseBoolError),

    /// [std::num::ParseFloatError]
    #[error(transparent)]
    ParseFloat(#[from] std::num::ParseFloatError),

    /// [std::num::ParseIntError]
    #[error(transparent)]
    ParseInt(#[from] std::num::ParseIntError),

    /// [jiff::Error]
    #[error(transparent)]
    ParseTimestamp(#[from] jiff::Error),

    /// A timestamp whose UTC offset is a day or more.
    ///
    /// RFC 3339 ends an offset at `23:59`. `jiff` reads one out to `25:59:59`, so it reads
    /// `2020-01-01T00:00:00+25:00` as `2019-12-30T23:00:00Z`.
    #[error("{0} has a UTC offset of 24 hours or more")]
    TimestampOffsetOutOfRange(String),

    /// [pest::error::Error]
    #[error(transparent)]
    Pest(#[from] Box<pest::error::Error<crate::parser::Rule>>),

    /// [serde_json::Error]
    #[error(transparent)]
    SerdeJson(#[from] serde_json::Error),

    /// A validation error.
    ///
    /// This holds a [serde_json::Value] that is the output from a
    /// [jsonschema::ValidationError]. We can't hold the validation error itself
    /// because it borrows from both the validated object and the validator's
    /// data.
    #[error("validation error")]
    Validation(serde_json::Value),

    /// Error Converting Expr to f64
    #[error("Could not convert Expression to f64")]
    ExprToF64(Expr),

    /// Error Converting Expr to bool
    #[error("Could not convert Expression to bool")]
    ExprToBool(Expr),

    /// Error Converting Expr to geometry
    #[error("Could not convert Expression to Geometry")]
    ExprToGeom(Expr),

    /// Error Converting Expr to DateRange
    #[error("Could not convert Expression to DateRange")]
    ExprToDateRange(Expr),

    /// Operator not implemented.
    #[error("Operator {0} is not implemented for this type.")]
    OpNotImplemented(&'static str),

    /// Invalid operator
    #[error("{0} is not a valid operator.")]
    InvalidOperator(String),

    /// Expression not reduced to boolean
    #[error("Could not reduce expression to boolean")]
    NonReduced(),

    /// Could not run arith operation
    #[error("Could not run operation.")]
    OperationError(),

    /// A name with no SQL spelling.
    ///
    /// An empty identifier prints as nothing at all, which would turn a predicate into a fragment.
    #[error("an empty name cannot be rendered as a SQL identifier")]
    EmptySqlIdentifier,

    /// A `GEOMETRYCOLLECTION` holding another `GEOMETRYCOLLECTION`.
    ///
    /// WKT allows the nesting; CQL2 does not. The cql2-json schema gives a collection's members as
    /// the six non-collection geometry types, so a nested collection has no cql2-json encoding and
    /// therefore no CQL2 expression.
    #[error("a GEOMETRYCOLLECTION cannot contain another GEOMETRYCOLLECTION: CQL2 admits only POINT, LINESTRING, POLYGON, MULTIPOINT, MULTILINESTRING and MULTIPOLYGON as members")]
    NestedGeometryCollection,

    /// A number with no cql2-text spelling.
    ///
    /// cql2-text has no literal for an infinity or a NaN, and `f64::to_string` writes them as the
    /// bare words `inf`, `-inf` and `NaN`, which the grammar reads back as *property* names: the
    /// rendering of `1 / 0` would parse as a reference to a column called `inf`. Both values are
    /// reachable from an expression that parsed — `1 / 0` reduces to an infinity and `0 / 0` to a
    /// NaN — and there is nothing correct to emit for them, so rendering one is an error.
    ///
    /// SQL is not affected: `to_sql` writes them as `CAST('Infinity' AS DOUBLE)` and friends.
    #[error("{0} has no cql2-text spelling: cql2-text has no literal for an infinity or a NaN")]
    NonFiniteNumber(f64),

    /// [json_dotpath::Error]
    #[error(transparent)]
    JsonDotpath(#[from] json_dotpath::Error),

    /// [like::InvalidPatternError]
    #[error(transparent)]
    Like(#[from] like::InvalidPatternError),
}
