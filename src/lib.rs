// Bounds struct derives PartialEq, but not Eq,
// so all containing types must also derive PartialEq without Eq
#![allow(clippy::derive_partial_eq_without_eq)]

pub mod config;
pub mod pg;
pub mod source;
pub mod srv;
pub mod utils;

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
