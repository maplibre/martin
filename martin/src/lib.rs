#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![forbid(unsafe_code)]

pub mod config;
pub mod logging;

#[cfg(feature = "_tiles")]
mod source;
#[cfg(feature = "_tiles")]
pub use source::TileSources;

#[cfg(feature = "_tiles")]
mod reload;
#[cfg(feature = "_tiles")]
pub use reload::{DeletedSource, NewSource, ReloadAdvisory};

#[cfg(feature = "_tiles")]
mod tile_source_manager;
#[cfg(feature = "_tiles")]
pub use tile_source_manager::TileSourceManager;

mod error;
pub use error::{MartinError, MartinResult};

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
