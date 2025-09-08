#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(missing_debug_implementations)]

/// Configuration utilities.
pub mod config;

/// Tile sources
pub mod tiles;

#[cfg(any(feature = "fonts", feature = "sprites", feature = "styles"))]
mod resources;
#[cfg(any(feature = "fonts", feature = "sprites", feature = "styles"))]
pub use resources::*;
