#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(missing_debug_implementations)]

/// Configuration utilities.
pub mod config;

/// Tile sources
pub mod tiles;

#[cfg(any(feature = "fonts", feature = "sprites"))]
mod resources;
#[cfg(any(feature = "fonts", feature = "sprites"))]
pub use resources::*;
