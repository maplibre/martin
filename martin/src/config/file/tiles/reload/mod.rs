#[cfg(feature = "unstable-cog")]
pub mod cog;
#[cfg(feature = "unstable-duckdb")]
pub mod duckdb;
#[cfg(feature = "geojson")]
pub mod geojson;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
#[cfg(feature = "pmtiles")]
pub mod pmtiles;
#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(any(
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "unstable-duckdb",
    feature = "geojson",
    feature = "pmtiles",
    feature = "postgres"
))]
mod reloaders;

#[cfg(any(
    feature = "mbtiles",
    feature = "unstable-cog",
    feature = "unstable-duckdb",
    feature = "geojson",
    feature = "pmtiles",
    feature = "postgres"
))]
pub use reloaders::TileReloaders;
