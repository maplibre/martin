mod error;
pub use crate::config::file::tiles::error::{SourceBuildError, SourceBuildResult};

#[cfg(feature = "cog")]
pub mod cog;
#[cfg(feature = "unstable-duckdb")]
pub mod duckdb;
#[cfg(feature = "processing")]
pub mod geojson;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
#[cfg(feature = "passthrough")]
pub mod passthrough;
#[cfg(feature = "pmtiles")]
pub mod pmtiles;
#[cfg(feature = "postgres")]
pub mod postgres;

#[cfg(feature = "_tiles")]
pub mod discovery;
#[cfg(feature = "_tiles")]
pub mod driver;

pub mod reload;
