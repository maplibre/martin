use std::path::PathBuf;

use duckdb::Error as DuckdbError;
use martin_core::tiles::duckdb::DuckDBError;

pub type BoundsResult<T> = Result<T, BoundsError>;
pub type GeoparquetResult<T> = Result<T, GeoparquetError>;

/// Errors raised while computing `DuckDB` bounds during config resolution.
#[derive(thiserror::Error, Debug)]
pub enum BoundsError {
    /// A bounds query failed to execute.
    #[error("Error computing DuckDB bounds for '{relation}' ({signature}): {query} {source}")]
    Query {
        /// The underlying `DuckDB` error.
        #[source]
        source: Box<DuckdbError>,
        /// Relation whose bounds were being computed.
        relation: String,
        /// Query signature (e.g. `approx-bounds` or `bounds`).
        signature: String,
        /// The SQL query that failed.
        query: String,
    },

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
        "GeoParquet source has multiple geometry columns ({columns:?}); set geometry_column explicitly"
    )]
    AmbiguousGeometryColumn { columns: Vec<String> },

    /// The configured geometry column does not exist.
    #[error("GeoParquet geometry column '{column}' was not found")]
    GeometryColumnNotFound { column: String },

    /// The configured geometry column is not a geometry type.
    #[error("GeoParquet column '{column}' is not a geometry column (type {column_type})")]
    NotGeometryColumn { column: String, column_type: String },

    /// The configured id column does not exist.
    #[error("GeoParquet id_column '{column}' was not found")]
    IdColumnNotFound { column: String },

    /// A local GeoParquet path cannot be represented as UTF-8 for SQL usage.
    #[error("GeoParquet path is not valid UTF-8: {path:?}")]
    NonUtf8Path { path: PathBuf },

    /// SRID could not be determined from metadata or sampled geometries.
    #[error("Unable to determine SRID for GeoParquet geometry column '{geometry_column}'")]
    SridUnknown { geometry_column: String },

    /// An introspection query failed to execute.
    #[error(
        "Error introspecting GeoParquet source '{source_label}' ({signature}): {query} {detail}"
    )]
    IntrospectionQuery {
        /// Human-readable error detail from `DuckDB`.
        detail: String,
        /// Path, URL, or label of the source being introspected.
        source_label: String,
        /// Query signature (e.g. `columns` or `srid`).
        signature: String,
        /// The SQL query that failed.
        query: String,
    },

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
        Self::IntrospectionQuery {
            detail: source.to_string(),
            source_label,
            signature: signature.to_string(),
            query,
        }
    }
}
