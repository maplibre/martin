//! A resolved tile-source description produced by `discover`, before it is instantiated into a running [`PostgresSource`].

use martin_core::tiles::postgres::PostgresSqlInfo;

use crate::config::file::postgres::{FunctionInfo, TableInfo};

/// A resolved tile-source description: catalog metadata merged with config and the id already resolved, ready to be instantiated into a running source.
#[derive(Clone, Debug)]
pub enum SourceSpec {
    /// A table source. Its SQL query and bounds are deferred to instantiate.
    Table(TableInfo),
    /// A function source. Its SQL is already produced by the catalog query.
    Function(FunctionInfo, PostgresSqlInfo),
}
