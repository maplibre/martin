//! Supporting Resource management for the Martin map tile server.
//!
//! Provides:
//! - [x] fonts
//! - [x] sprites
//! - [x] styles

#[cfg(feature = "resources")]
pub mod fonts;

#[cfg(feature = "resources")]
pub mod sprites;

#[cfg(feature = "resources")]
pub mod styles;

#[cfg(feature = "resources")]
mod walk;
#[cfg(feature = "resources")]
pub use walk::walk_files;
