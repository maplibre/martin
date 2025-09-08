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
