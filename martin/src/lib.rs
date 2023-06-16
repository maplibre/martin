#![warn(clippy::pedantic)]
// Bounds struct derives PartialEq, but not Eq,
// so all containing types must also derive PartialEq without Eq
#![allow(clippy::derive_partial_eq_without_eq)]
#![allow(clippy::implicit_hasher)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]

pub mod args;
mod config;
pub mod file_config;
pub mod mbtiles;
pub mod pg;
pub mod pmtiles;
mod source;
pub mod sprites;
pub mod srv;
mod utils;

#[cfg(test)]
#[path = "utils/test_utils.rs"]
mod test_utils;

// test_utils is used from tests in other modules, and it uses this crate's object.
// Must make it accessible as carte::Env from both places when testing.
#[cfg(test)]
pub use crate::args::Env;
pub use crate::config::{read_config, Config};
pub use crate::source::{Source, Sources, Xyz};
pub use crate::utils::{
    decode_brotli, decode_gzip, BoolOrObject, Error, IdResolver, OneOrMany, Result,
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
