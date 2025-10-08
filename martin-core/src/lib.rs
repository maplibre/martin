#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]

/// Configuration utilities.
pub mod config;

/// Tile sources
#[cfg(feature = "_tiles")]
pub mod tiles;

/// Cache utilities.
pub mod cache;

#[cfg(any(feature = "fonts", feature = "sprites", feature = "styles"))]
mod resources;
#[cfg(any(feature = "fonts", feature = "sprites", feature = "styles"))]
pub use resources::*;
