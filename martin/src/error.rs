use std::fmt::Write as _;
use std::io;

/// A convenience [`Result`] for Martin crate.
pub type MartinResult<T> = Result<T, MartinError>;

fn elide_vec(vec: &[String], max_items: usize, max_len: usize) -> String {
    let mut s = String::new();
    for (i, v) in vec.iter().enumerate() {
        if i > max_items {
            let _ = write!(s, " and {} more", vec.len() - i);
            break;
        }
        if i > 0 {
            s.push(' ');
        }
        if v.len() > max_len {
            s.push_str(&v[..max_len]);
            s.push('…');
        } else {
            s.push_str(v);
        }
    }
    s
}

#[derive(thiserror::Error, Debug)]
pub enum MartinError {
    #[error("The --config and the connection parameters cannot be used together. Please remove unsupported parameters '{}'", elide_vec(.0, 3, 15))]
    ConfigAndConnectionsError(Vec<String>),

    #[error("Unable to bind to {1}: {0}")]
    BindingError(#[source] io::Error, String),

    #[error("Base path must be a valid URL path, and must begin with a '/' symbol, but is '{0}'")]
    BasePathError(String),

    #[error("Unrecognizable connection strings: {0:?}")]
    UnrecognizableConnections(Vec<String>),

    #[cfg(feature = "postgres")]
    #[error(transparent)]
    PostgresError(#[from] martin_core::tiles::postgres::PostgresError),

    #[cfg(feature = "pmtiles")]
    #[error(transparent)]
    PmtilesError(#[from] martin_core::tiles::pmtiles::PmtilesError),

    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    MbtilesError(#[from] martin_core::tiles::mbtiles::MbtilesError),

    #[cfg(feature = "unstable-cog")]
    #[error(transparent)]
    CogError(#[from] martin_core::tiles::cog::CogError),

    #[cfg(feature = "geojson")]
    #[error(transparent)]
    GeoJsonError(#[from] martin_core::tiles::geojson::GeoJsonError),

    #[error(transparent)]
    ConfigFileError(#[from] crate::config::file::ConfigFileError),

    #[cfg(feature = "sprites")]
    #[error(transparent)]
    SpriteError(#[from] martin_core::sprites::SpriteError),

    #[error(transparent)]
    WebError(#[from] actix_web::Error),

    #[error(transparent)]
    IoError(#[from] io::Error),

    #[cfg(feature = "lambda")]
    #[error(transparent)]
    LambdaError(#[from] lambda_web::LambdaError),

    #[cfg(feature = "metrics")]
    #[error("could not initialize metrics: {0}")]
    MetricsIntialisationError(#[source] Box<dyn std::error::Error>),

    #[error("warnings issued during tile source resolution")]
    TileResolutionWarningsIssued,
}
