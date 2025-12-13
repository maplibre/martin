#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![forbid(unsafe_code)]

pub mod config;

#[cfg(feature = "_tiles")]
mod source;
#[cfg(feature = "_tiles")]
pub use source::TileSources;

mod error;
pub use error::{MartinError, MartinResult, TileSourceWarning};

pub mod srv;

// Ensure README.md contains valid code
#[cfg(doctest)]
mod test_readme {
    macro_rules! external_doc_test {
        ($x:expr) => {
            #[doc = $x]
            unsafe extern "C" {}
        };
    }

    external_doc_test!(include_str!("../README.md"));
}
