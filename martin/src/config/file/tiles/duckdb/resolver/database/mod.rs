mod discover;
mod resolve;

pub use crate::config::file::tiles::duckdb::resolver::database::resolve::{
    resolve_database_entry, resolve_macro_source, resolve_table_source,
};
