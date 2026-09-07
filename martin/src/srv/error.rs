//! Failures from bringing the HTTP server up.
//!
//! Reachable only from [`new_server`](super::new_server), whose single caller is
//! `bin/martin.rs`. Separate from [`StartupError`](crate::StartupError) so config
//! resolution and source construction can neither produce nor match on a bind failure.

use std::io;
#[cfg(feature = "_tiles")]
use std::sync::Arc;

#[cfg(feature = "_tiles")]
use martin_core::tiles::MartinCoreError;
#[cfg(feature = "_tiles")]
use martin_tile_utils::{Encoding, Format, TileInfo};

use crate::config::file::ConfigFileError;
#[cfg(feature = "_tiles")]
use crate::srv::tiles::process::ProcessError;

/// Why the HTTP server could not be started.
#[derive(thiserror::Error, Debug)]
pub enum ServerStartError {
    #[error("Unable to bind to {1}: {0}")]
    Binding(#[source] io::Error, String),

    #[cfg(feature = "lambda")]
    #[error(transparent)]
    Lambda(#[from] lambda_web::LambdaError),

    #[cfg(feature = "metrics")]
    #[error("could not initialize metrics: {0}")]
    MetricsInitialisation(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// The sprite catalog could not be built while assembling the server's catalog.
    #[cfg(feature = "sprites")]
    #[error(transparent)]
    SpriteCatalog(#[from] martin_core::sprites::SpriteError),

    /// The CORS block in the config was rejected while configuring the server.
    #[error(transparent)]
    Cors(#[from] ConfigFileError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Why a tile could not be produced.
#[cfg(feature = "_tiles")]
#[derive(thiserror::Error, Debug)]
pub enum TileError {
    /// No source is registered under this id.
    #[error("Source {0} does not exist")]
    UnknownSource(String),

    /// A composite request named more sources than one request may carry.
    #[error("Requested {requested} source ids, but at most {max} are allowed per request")]
    TooManySources { requested: usize, max: usize },

    /// A composite request mixed sources that do not share a tile format.
    #[error("Cannot merge sources with {left} with {right}")]
    MismatchedSources { left: TileInfo, right: TileInfo },

    /// Every source the request named sits outside the requested zoom.
    #[error("Zoom {zoom} is outside the supported range: {supported}")]
    ZoomOutOfRange { zoom: u8, supported: String },

    /// The `Accept` header names no format these sources can produce.
    #[error("Source produces {}, which does not match the Accept header", .0.content_type())]
    UnacceptableFormat(Format),

    /// The request's query string is not a flat set of key/value pairs.
    #[error(transparent)]
    InvalidQuery(#[from] actix_web::error::QueryPayloadError),

    /// The `Accept-Encoding` header names no encoding this server can produce.
    #[error("No supported encoding found")]
    NoAcceptableEncoding,

    /// The tile is stored in an encoding that has no decoder, so it cannot be re-encoded.
    #[error("Tile is stored as {0}, but the client does not accept this encoding")]
    UndecodableEncoding(TileInfo),

    /// The resolved tiles are not concatenable vector tiles, so they cannot be merged.
    #[error(
        "Cannot merge non-vector-tile formats. Format is {format:?} with encoding {encoding:?} "
    )]
    UnmergeableTiles { format: Format, encoding: Encoding },

    /// The source itself failed; it classifies its own status.
    #[error("{0}")]
    Source(Arc<MartinCoreError>),

    /// Post-processing (hillshade, contour, MVT/MLT) failed; it classifies its own status.
    #[cfg(feature = "_tiles")]
    #[error(transparent)]
    Process(#[from] ProcessError),

    /// The tile body could not be (de)compressed.
    #[error("Tile compression failed: {0}")]
    Compression(#[from] io::Error),
}

#[cfg(feature = "_tiles")]
impl From<MartinCoreError> for TileError {
    fn from(e: MartinCoreError) -> Self {
        Self::Source(Arc::new(e))
    }
}

#[cfg(feature = "_tiles")]
impl From<Arc<MartinCoreError>> for TileError {
    fn from(e: Arc<MartinCoreError>) -> Self {
        Self::Source(e)
    }
}

#[cfg(feature = "_tiles")]
impl From<TileError> for actix_web::Error {
    fn from(e: TileError) -> Self {
        use actix_web::error::{
            ErrorBadRequest, ErrorInternalServerError, ErrorNotAcceptable, ErrorNotFound,
        };

        let msg = e.to_string();
        match e {
            TileError::UnknownSource(_)
            | TileError::MismatchedSources { .. }
            | TileError::ZoomOutOfRange { .. } => ErrorNotFound(msg),

            TileError::TooManySources { .. }
            | TileError::InvalidQuery(_)
            | TileError::UnmergeableTiles { .. }
            | TileError::UndecodableEncoding(_) => ErrorBadRequest(msg),

            TileError::UnacceptableFormat(_) | TileError::NoAcceptableEncoding => {
                ErrorNotAcceptable(msg)
            }

            TileError::Compression(ref inner) => {
                tracing::error!("{inner}");
                ErrorInternalServerError(msg)
            }

            // Both of these already classify themselves, so reuse their own mapping.
            TileError::Source(inner) => super::server::map_error(inner.as_ref()),
            #[cfg(feature = "_tiles")]
            TileError::Process(inner) => inner.into(),
        }
    }
}
