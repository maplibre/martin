//! Static-render overlays: the typed boundary IR a rendered base map is
//! decorated with.
//!
//! [`OverlaySpec`] is a pre-validated `GeoJSON` `FeatureCollection`. The wire
//! format -- CSS-color strings, the `FeatureCollection` envelope -- is an
//! application concern, so the `martin` crate owns deserialization and builds
//! these types from a request body; martin-core only ever sees the already-valid
//! IR. The geometry->layer fan-out is a rendering concern, applied at render time.

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
