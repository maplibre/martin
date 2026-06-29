use std::path::PathBuf;

use duckdb::Error as DuckdbError;
use martin_core::tiles::duckdb::DuckDBError;

pub type BoundsResult<T> = Result<T, BoundsError>;
pub type GeoparquetResult<T> = Result<T, GeoparquetError>;

/// Errors raised while computing `DuckDB` bounds during config resolution.
#[derive(thiserror::Error, Debug)]
pub enum BoundsError {
    /// A bounds query failed to execute.
    #[error("Error computing DuckDB bounds for '{1}' ({2}): {3} {0}")]
    Query(#[source] Box<DuckdbError>, String, String, String),

    /// An error from the shared `DuckDB` pool or blocking task runtime.
    #[error(transparent)]
    Pool(#[from] DuckDBError),
}

/// Errors raised while resolving GeoParquet tile sources.
#[derive(thiserror::Error, Debug)]
pub enum GeoparquetError {
    /// No geometry column was found in the GeoParquet file.
    #[error("GeoParquet source has no geometry column")]
    NoGeometryColumn,

    /// Multiple geometry columns were found without an explicit `geometry_column`.
    #[error(
        "GeoParquet source has multiple geometry columns ({0:?}); set geometry_column explicitly"
    )]
    AmbiguousGeometryColumn(Vec<String>),

    /// The configured geometry column does not exist.
    #[error("GeoParquet geometry column '{0}' was not found")]
    GeometryColumnNotFound(String),

    /// The configured geometry column is not a geometry type.
    #[error("GeoParquet column '{0}' is not a geometry column (type {1})")]
    NotGeometryColumn(String, String),

    /// The configured id column does not exist.
    #[error("GeoParquet id_column '{0}' was not found")]
    IdColumnNotFound(String),

    /// A local GeoParquet path cannot be represented as UTF-8 for SQL usage.
    #[error("GeoParquet path is not valid UTF-8: {0:?}")]
    NonUtf8Path(PathBuf),

    /// `ST_CRS` returned no CRS for the geometry column.
    #[error("Unable to determine SRID for GeoParquet geometry column '{0}'")]
    SridUnknown(String),

    /// CRS string from `ST_CRS` was empty.
    #[error("GeoParquet geometry column '{0}' has an empty CRS string")]
    SridEmpty(String, String),

    /// CRS authority is not EPSG or OGC:CRS84.
    #[error("GeoParquet geometry column '{0}' uses unsupported CRS '{1}'")]
    SridUnsupportedCrs(String, String),

    /// EPSG code is not a valid integer.
    #[error("GeoParquet geometry column '{0}' has invalid EPSG code in CRS '{1}'")]
    SridInvalidEpsgCode(String, String),

    /// Parsed EPSG code is zero or negative.
    #[error("GeoParquet geometry column '{0}' has non-positive EPSG code {2} in CRS '{1}'")]
    SridNonPositive(String, String, i32),

    /// An introspection query failed to execute.
    #[error("Error introspecting GeoParquet source '{1}' ({2}): {3} {0}")]
    IntrospectionQuery(String, String, String, String),

    /// An error from bounds calculation.
    #[error(transparent)]
    Bounds(#[from] BoundsError),

    /// An error from the shared `DuckDB` pool or blocking task runtime.
    #[error(transparent)]
    Pool(#[from] DuckDBError),
}

impl GeoparquetError {
    pub(crate) fn introspection_query(
        source: DuckdbError,
        source_label: String,
        signature: &'static str,
        query: String,
    ) -> Self {
        Self::IntrospectionQuery(
            source.to_string(),
            source_label,
            signature.to_string(),
            query,
        )
    }
}
