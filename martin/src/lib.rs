#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]

pub mod args;
mod config;
pub mod file_config;
pub mod fonts;
pub mod mbtiles;
pub mod pg;
pub mod pmtiles;
mod source;
pub mod sprites;
pub mod srv;
mod utils;
pub use utils::Xyz;

#[cfg(test)]
#[path = "utils/test_utils.rs"]
mod test_utils;

// test_utils is used from tests in other modules, and it uses this crate's object.
// Must make it accessible as carte::Env from both places when testing.
#[cfg(test)]
pub use crate::args::Env;
pub use crate::config::{read_config, Config, ServerState};
pub use crate::source::Source;
pub use crate::utils::{
    decode_brotli, decode_gzip, Error, IdResolver, OptBoolObj, OptOneMany, Result,
};

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
