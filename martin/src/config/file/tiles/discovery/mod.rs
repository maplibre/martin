//! The `Discovery` trait and its implementations: `FsDiscovery` for the file-backed kinds and
//! `ObjectStoreDiscovery` for remote `PMTiles` prefixes.

mod discovery_trait;
pub use discovery_trait::{BuiltSource, Discovered, Discovery, Version};

#[cfg(any(
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "geojson",
    feature = "pmtiles"
))]
mod fs;
#[cfg(any(
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "geojson",
    feature = "pmtiles"
))]
pub use fs::{FsDiscovery, FsSourceBuilder};

#[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
mod object_store;
#[cfg(feature = "unstable-cog")]
pub(crate) use object_store::version_from_cog_meta;
#[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
pub use object_store::{
    ConfiguredObjectDiscovery, ObjectStoreDiscovery, ObjectStoreParser, ObjectStoreSourceBuilder,
};

#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "postgres")]
pub use postgres::PostgresDiscovery;
