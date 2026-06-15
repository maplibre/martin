mod config;
pub use config::*;

mod config_geoparquet;
pub use config_geoparquet::{
    DuckDbGeoParquetSourceConfig, GeoParquetTarget, parse_geoparquet_target,
};

mod config_macro;
pub use config_macro::*;

mod config_table;
pub use config_table::*;

pub(crate) mod utils;

mod builder;
pub use builder::DuckDbAutoDiscoveryBuilder;

mod spec;
pub use spec::SourceSpec;

pub mod resolver;
