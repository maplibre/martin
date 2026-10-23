mod auto_publish;
mod database;
mod geoparquet;
mod layer;
mod settings;

pub(crate) use settings::DuckDbSourceDefaults;

pub use crate::config::file::tiles::duckdb::sources::auto_publish::{
    DuckDbCfgPublish, DuckDbCfgPublishMacros, DuckDbCfgPublishTables,
};
pub use crate::config::file::tiles::duckdb::sources::database::{
    DuckDbDatabaseEntry, DuckDbMacroEntry, DuckDbTableEntry,
};
pub use crate::config::file::tiles::duckdb::sources::geoparquet::{GeoParquetEntry, GeoParquetLocation};
pub use crate::config::file::tiles::duckdb::sources::layer::MvtLayerOptions;
pub use crate::config::file::tiles::duckdb::sources::settings::DuckDbSourceSettings;
