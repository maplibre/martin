mod errors;
pub use errors::{DuckDBError, DuckDBResult};

mod pool;
pub use pool::{DuckDBPool, GEOPARQUET_VIEW};
mod source;
pub use source::{DuckDBSource, DuckDBSqlInfo};
