//! Parse and transform [Common Query Language 2 (CQL2)](https://docs.ogc.org/is/21-065r2/21-065r2.html).

#![warn(missing_docs)]
#![deny(
    elided_lifetimes_in_paths,
    explicit_outlives_requirements,
    keyword_idents,
    macro_use_extern_crate,
    meta_variable_misuse,
    missing_abi,
    missing_debug_implementations,
    non_ascii_idents,
    noop_method_call,
    rust_2021_incompatible_closure_captures,
    rust_2021_incompatible_or_patterns,
    rust_2021_prefixes_incompatible_syntax,
    rust_2021_prelude_collisions,
    single_use_lifetimes,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_pub,
    unsafe_code,
    unsafe_op_in_unsafe_fn,
    unused_crate_dependencies,
    unused_extern_crates,
    unused_import_braces,
    unused_lifetimes,
    unused_qualifications,
    unused_results
)]
#![allow(clippy::result_large_err)]

mod duckdb;
mod error;
mod expr;
mod geometry;
mod parser;
mod precedence;
mod sql;
mod temporal;
mod validator;

pub use duckdb::ToDuckSQL;
pub use error::Error;
pub use expr::*;
pub use geometry::{spatial_op, Geometry};
pub use parser::parse_text;
pub use sql::ToSqlAst;
use std::{fs, path::Path};
pub use temporal::{temporal_op, DateRange};
pub use validator::Validator;

/// Parses a cql2-json string into a CQL2 expression.
///
/// # Examples
///
/// ```
/// let s = include_str!("../examples/json/example01.json");
/// let expr = cql2::parse_json(s);
/// ```
pub fn parse_json(s: &str) -> Result<Expr, serde_json::Error> {
    // Normalization happens in `Expr`'s `Deserialize` impl, so every serde entry point agrees.
    serde_json::from_str(s)
}

/// Reads a file and returns its contents as a CQL2 expression;
///
/// # Examples
///
/// ```no_run
/// let expr = cql2::parse_file("examples/json/example01.json");
/// ```
pub fn parse_file(path: impl AsRef<Path>) -> Result<Expr, Error> {
    let s = fs::read_to_string(path)?;
    s.parse()
}

#[cfg(test)]
use {::duckdb as _, assert_json_diff as _, criterion as _, proptest as _, rstest as _};

// From https://github.com/rust-lang/cargo/issues/383#issuecomment-720873790,
// may they be forever blessed.
#[cfg(doctest)]
mod readme {
    macro_rules! external_doc_test {
        ($x:expr) => {
            #[doc = $x]
            extern "C" {}
        };
    }

    external_doc_test!(include_str!("../README.md"));
}
