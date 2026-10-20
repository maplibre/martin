mod config;
pub use config::*;

mod config_function;
pub use config_function::*;

mod config_table;
pub use config_table::*;

pub(crate) mod utils;

mod builder;
pub use crate::config::file::tiles::postgres::builder::PostgresAutoDiscoveryBuilder;

mod id_format;
pub use crate::config::file::tiles::postgres::id_format::{
    FunctionIdFormat, FunctionIds, IdFormat, IdPlaceholders, TableIdFormat, TableIds, UnknownIdPlaceholder,
};

mod spec;
pub use crate::config::file::tiles::postgres::spec::SourceSpec;

mod srid;
pub use crate::config::file::tiles::postgres::srid::{Srid, SridOutOfRange};

mod tile_grid;
pub use crate::config::file::tiles::postgres::tile_grid::PgTileGrid;

pub mod resolver;
