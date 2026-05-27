//! The `Discovery` trait and its implementations: `FsDiscovery` for the file-backed kinds and
//! `ObjectStoreDiscovery` for remote `PMTiles` prefixes.

mod discovery_trait;
pub use discovery_trait::{Discovery, Version};

#[cfg(any(feature = "mbtiles", feature = "unstable-cog", feature = "pmtiles"))]
mod fs;
#[cfg(any(feature = "mbtiles", feature = "unstable-cog", feature = "pmtiles"))]
pub use fs::{FsDiscovery, FsSourceBuilder};

#[cfg(feature = "pmtiles")]
mod object_store;
#[cfg(feature = "pmtiles")]
pub use object_store::ObjectStoreDiscovery;

#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "postgres")]
pub use postgres::PostgresDiscovery;
