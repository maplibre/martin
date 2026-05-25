//! Supporting Resource management for the Martin map tile server.
//!
//! Provides:
//! - [x] fonts
//! - [x] sprites
//! - [x] styles

#[cfg(feature = "fonts")]
pub mod fonts;

#[cfg(feature = "sprites")]
pub mod sprites;

#[cfg(feature = "styles")]
pub mod styles;

#[cfg(any(feature = "fonts", feature = "sprites", feature = "styles"))]
mod walk;
#[cfg(any(feature = "fonts", feature = "sprites", feature = "styles"))]
pub use walk::walk_files;
