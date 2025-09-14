#![cfg_attr(doc, doc = include_str!("../README.md"))]
#![forbid(unsafe_code)]

pub mod config;

mod source;
pub use source::TileSources;

mod utils;
pub use utils::{IdResolver, MartinError, MartinResult, NO_MAIN_CACHE};

#[cfg(feature = "cog")]
pub mod cog;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
#[cfg(feature = "postgres")]
pub mod pg;
#[cfg(feature = "pmtiles")]
pub mod pmtiles;
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
