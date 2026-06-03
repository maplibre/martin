//! The `Discovery` trait and its implementations: `FsDiscovery` for the file-backed kinds.

mod discovery_trait;
pub use discovery_trait::{Discovery, Version};

#[cfg(any(feature = "mbtiles", feature = "unstable-cog", feature = "pmtiles"))]
mod fs;
#[cfg(any(feature = "mbtiles", feature = "unstable-cog", feature = "pmtiles"))]
pub use fs::{FsDiscovery, FsSourceBuilder};
