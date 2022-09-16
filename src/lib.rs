pub mod composite_source;
pub mod config;
pub mod db;
pub mod dev;
pub mod function_source;
pub mod server;
pub mod source;
pub mod table_source;
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
