use std::error::Error;
use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;

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
    BindingError(io::Error, String),

    #[error("Base path must be a valid URL path, and must begin with a '/' symbol, but is '{0}'")]
    BasePathError(String),

    #[error("Unable to load config file {1}: {0}")]
    ConfigLoadError(io::Error, PathBuf),

    #[error("Unable to parse config file {1}: {0}")]
    ConfigParseError(subst::yaml::Error, PathBuf),

    #[error("Unable to write config file {1}: {0}")]
    ConfigWriteError(io::Error, PathBuf),

    #[error("No tile sources found. Set sources by giving a database connection string on command line, env variable, or a config file.")]
    NoSources,

    #[error("Unrecognizable connection strings: {0:?}")]
    UnrecognizableConnections(Vec<String>),

    #[cfg(feature = "postgres")]
    #[error(transparent)]
    PostgresError(#[from] crate::pg::PgError),

    #[cfg(feature = "pmtiles")]
    #[error(transparent)]
    PmtilesError(#[from] pmtiles::PmtError),

    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    MbtilesError(#[from] mbtiles::MbtError),

    #[cfg(feature = "cog")]
    #[error(transparent)]
    CogError(#[from] crate::cog::CogError),

    #[error(transparent)]
    FileError(#[from] crate::file_config::FileError),

    #[cfg(feature = "sprites")]
    #[error(transparent)]
    SpriteError(#[from] crate::sprites::SpriteError),

    #[cfg(feature = "fonts")]
    #[error(transparent)]
    FontError(#[from] crate::fonts::FontError),

    #[error(transparent)]
    WebError(#[from] actix_web::Error),

    #[error(transparent)]
    IoError(#[from] io::Error),

    #[error("Internal error: {0}")]
    InternalError(#[from] Box<dyn Error + Send + Sync>),
}
