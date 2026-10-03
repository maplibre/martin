//! Supporting Resource management for the Martin map tile server.
//!
//! Provides:
//! - [x] fonts
//! - [x] sprites
//! - [x] styles

pub mod fonts;
pub mod sprites;
pub mod styles;

mod walk;
pub use walk::walk_files;
