pub mod bounds;
pub mod database;
pub mod errors;
pub mod geoparquet;
mod introspect;
mod metadata;
mod mvt_types;
mod resolve;
mod secret;
mod sql;

pub use crate::config::file::tiles::duckdb::resolver::bounds::bounds_with_auto;
pub use crate::config::file::tiles::duckdb::resolver::errors::{
    BoundsError, BoundsResult, DuckDbSourceError, DuckDbSourceResult,
};
pub use crate::config::file::tiles::duckdb::resolver::geoparquet::resolve_geoparquet_source;
pub use crate::config::file::tiles::duckdb::resolver::introspect::LayerIntrospection;
pub use crate::config::file::tiles::duckdb::resolver::sql::build_mvt_sql;
