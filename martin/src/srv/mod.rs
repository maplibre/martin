mod config;
pub use config::{SrvConfig, KEEP_ALIVE_DEFAULT, LISTEN_ADDRESSES_DEFAULT};

mod server;
pub use server::{
    get_tile_content, get_tile_impl, merge_tilejson, new_server, router, Catalog, TileRequest,
    RESERVED_KEYWORDS,
};
