mod introspect;
mod metadata;
mod resolve;
mod sql;

#[cfg(all(test, feature = "unstable-duckdb"))]
mod tests;

pub use introspect::GeoParquetIntrospection;
pub use resolve::resolve_geoparquet_source;
pub use sql::build_mvt_sql;
