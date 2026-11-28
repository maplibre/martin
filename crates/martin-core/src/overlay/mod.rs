//! Allows decorating a rendered base map with overlays.

mod model;
pub use model::*;

#[cfg(feature = "rendering")]
mod error;
#[cfg(feature = "rendering")]
pub use error::ApplyError;

#[cfg(feature = "rendering")]
mod apply;
#[cfg(feature = "rendering")]
pub use apply::{AppliedOverlay, apply_to_style};
