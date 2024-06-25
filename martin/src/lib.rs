#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

mod config;
pub use config::{read_config, Config, ServerState};

mod source;
pub use source::{CatalogSourceEntry, Source, Tile, TileData, TileSources, UrlQuery};

mod utils;
pub use utils::{
    append_rect, IdResolver, MartinError, MartinResult, OptBoolObj, OptOneMany, TileCoord,
    TileRect, NO_MAIN_CACHE,
};

pub mod args;
pub mod file_config;
#[cfg(feature = "fonts")]
pub mod fonts;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
#[cfg(feature = "postgres")]
pub mod pg;
#[cfg(feature = "pmtiles")]
pub mod pmtiles;
#[cfg(feature = "sprites")]
pub mod sprites;
pub mod srv;

#[cfg(test)]
#[path = "utils/test_utils.rs"]
mod test_utils;

// test_utils is used from tests in other modules, and it uses this crate's object.
// Must make it accessible as carte::Env from both places when testing.
#[cfg(test)]
pub use crate::args::Env;

// Ensure README.md contains valid code
#[cfg(doctest)]
mod test_readme {
    macro_rules! external_doc_test {
        ($x:expr) => {
            #[doc = $x]
            extern "C" {}
        };
    }

    external_doc_test!(include_str!("../README.md"));
}
