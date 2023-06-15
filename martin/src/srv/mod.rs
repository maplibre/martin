mod config;
mod server;

pub use config::{SrvConfig, KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT};
pub use server::{new_server, router, RESERVED_KEYWORDS};

pub use crate::source::IndexEntry;
