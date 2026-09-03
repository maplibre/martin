//! Failures from constructing a tile source.
//!
//! The error of the *build* seam - [`Discovery`](super::discovery::Discovery) and the
//! per-backend resolvers. Startup lifts it into [`StartupError`](crate::StartupError) and
//! renders it; hot reload logs it and retains the previous source set. Runtime tile *fetch*
//! is a different seam - see [`MartinCoreError`](martin_core::tiles::MartinCoreError).

use std::io;
use std::path::PathBuf;

#[cfg(feature = "unstable-cog")]
use martin_core::tiles::cog::CogError;
#[cfg(feature = "geojson")]
use martin_core::tiles::geojson::GeoJsonError;
#[cfg(feature = "mbtiles")]
use martin_core::tiles::mbtiles::MbtilesError;
#[cfg(feature = "passthrough")]
use martin_core::tiles::passthrough::PassthroughError;
#[cfg(feature = "pmtiles")]
use martin_core::tiles::pmtiles::PmtilesError;
#[cfg(feature = "postgres")]
use martin_core::tiles::postgres::PostgresError;

use crate::config::file::ConfigFileError;

/// A convenience [`Result`] for the tile source build seam.
pub type SourceBuildResult<T> = Result<T, SourceBuildError>;

/// Why a tile source could not be constructed.
#[derive(thiserror::Error, Debug)]
pub enum SourceBuildError {
    #[cfg(feature = "postgres")]
    #[error(transparent)]
    Postgres(#[from] PostgresError),

    #[cfg(feature = "pmtiles")]
    #[error(transparent)]
    Pmtiles(#[from] PmtilesError),

    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    Mbtiles(#[from] MbtilesError),

    #[cfg(feature = "passthrough")]
    #[error(transparent)]
    Passthrough(#[from] PassthroughError),

    #[cfg(feature = "unstable-cog")]
    #[error(transparent)]
    Cog(#[from] CogError),

    #[cfg(feature = "geojson")]
    #[error(transparent)]
    GeoJson(#[from] GeoJsonError),

    /// The source's configuration was rejected while building it.
    #[error(transparent)]
    Config(#[from] ConfigFileError),

    #[error("IO error while building tile sources: {0}")]
    Io(#[from] io::Error),

    /// A reload advisory named a source that discovery no longer reports.
    #[error("Source '{0}' not found in discovered sources")]
    SourceNotFound(String),

    #[error("Source path is not a file: {0}")]
    InvalidFilePath(PathBuf),
}
