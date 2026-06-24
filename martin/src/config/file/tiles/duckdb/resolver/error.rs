use duckdb::Error as DuckdbError;
use martin_core::tiles::duckdb::DuckDBError;

pub type BoundsResult<T> = Result<T, BoundsError>;

/// Errors raised while computing DuckDB relation bounds during config resolution.
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
    DuckDB(#[from] DuckDBError),
}
