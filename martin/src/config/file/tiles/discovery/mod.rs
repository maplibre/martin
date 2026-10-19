//! The `Discovery` trait and its implementations: `FsDiscovery` for file-backed kinds,
//! configured remote-object discovery, and object-store prefix discovery.

mod discovery_trait;
pub use crate::config::file::tiles::discovery::discovery_trait::{
    BuiltSource, Discovered, Discovery, Version,
};

#[cfg(any(
    feature = "mbtiles",
    feature = "cog",
    feature = "processing",
    feature = "pmtiles"
))]
mod fs;
#[cfg(any(
    feature = "mbtiles",
    feature = "cog",
    feature = "processing",
    feature = "pmtiles"
))]
pub use crate::config::file::tiles::discovery::fs::{FsDiscovery, FsSourceBuilder};

#[cfg(any(feature = "pmtiles", feature = "cog"))]
mod object_store;
#[cfg(any(feature = "pmtiles", feature = "cog"))]
pub use crate::config::file::tiles::discovery::object_store::{
    ConfiguredObjectDiscovery, ObjectStoreDiscovery, ObjectStoreParser, ObjectStoreSourceBuilder,
};

#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "postgres")]
pub use crate::config::file::tiles::discovery::postgres::PostgresDiscovery;
