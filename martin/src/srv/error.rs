//! Failures from bringing the HTTP server up.
//!
//! Reachable only from [`new_server`](super::new_server), whose single caller is
//! `bin/martin.rs`. Separate from [`StartupError`](crate::StartupError) so config
//! resolution and source construction can neither produce nor match on a bind failure.

use std::io;

use crate::config::file::ConfigFileError;

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
