#[cfg(feature = "cog")]
pub mod cog;
#[cfg(feature = "unstable-duckdb")]
pub mod duckdb;
#[cfg(feature = "processing")]
pub mod geojson;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
#[cfg(feature = "pmtiles")]
pub mod pmtiles;
#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(any(
    feature = "mbtiles",
    feature = "cog",
    feature = "unstable-duckdb",
    feature = "processing",
    feature = "pmtiles",
    feature = "postgres"
))]
mod reloaders;

#[cfg(any(
    feature = "mbtiles",
    feature = "cog",
    feature = "unstable-duckdb",
    feature = "processing",
    feature = "pmtiles",
    feature = "postgres"
))]
pub use crate::config::file::tiles::reload::reloaders::TileReloaders;
