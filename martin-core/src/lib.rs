#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(missing_debug_implementations)]

mod error;
pub use crate::error::{Classify, ErrorKind};

/// Generic resource cache shared by sprite, font, and tile caches.
pub mod cache;

mod cache_zoom_range;
pub use crate::cache_zoom_range::CacheZoomRange;

#[cfg(any(feature = "rendering", feature = "cog"))]
mod image_quality;
#[cfg(any(feature = "rendering", feature = "cog"))]
pub use crate::image_quality::{ImageQuality, LossyQuality};

#[cfg(feature = "metrics")]
pub mod metrics;

/// Tile sources
#[cfg(feature = "_tiles")]
pub mod tiles;

#[cfg(feature = "resources")]
mod resources;
#[cfg(feature = "resources")]
pub use resources::*;

/// Draw vector overlays (paths, markers) onto a rendered raster.
#[cfg(feature = "overlay")]
pub mod overlay;
