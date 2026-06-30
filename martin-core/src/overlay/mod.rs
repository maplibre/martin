//! Allows decorating a rendered base map with overlays.

mod model;
pub use model::*;

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod error;
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use error::ApplyError;

#[cfg(all(feature = "rendering", target_os = "linux"))]
mod apply;
#[cfg(all(feature = "rendering", target_os = "linux"))]
pub use apply::{AppliedOverlay, apply_to_style};
