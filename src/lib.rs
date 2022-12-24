// Bounds struct derives PartialEq, but not Eq,
// so all containing types must also derive PartialEq without Eq
#![allow(clippy::derive_partial_eq_without_eq)]
#![allow(clippy::implicit_hasher)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::module_name_repetitions)]

pub mod args;
pub mod config;
pub mod pg;
pub mod source;
pub mod srv;
pub mod utils;

pub use crate::utils::Error;
pub use crate::utils::Result;

// test_utils is used from tests in other modules, and it uses this crate's object.
// Must make it accessible as carte::Env from both places when testing.
#[cfg(test)]
pub use crate::args::environment::Env;
#[cfg(test)]
#[path = "utils/test_utils.rs"]
mod test_utils;

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
