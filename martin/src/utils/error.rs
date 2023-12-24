use std::error::Error;
use std::fmt::Write as _;
use std::io;
use std::path::PathBuf;

use mbtiles::MbtError;

use crate::file_config::FileError;
#[cfg(feature = "fonts")]
use crate::fonts::FontError;
use crate::pg::PgError;
#[cfg(feature = "sprites")]
use crate::sprites::SpriteError;

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

    #[error("Unable to load config file {}: {0}", .1.display())]
    ConfigLoadError(io::Error, PathBuf),

    #[error("Unable to parse config file {}: {0}", .1.display())]
    ConfigParseError(subst::yaml::Error, PathBuf),

    #[error("Unable to write config file {}: {0}", .1.display())]
    ConfigWriteError(io::Error, PathBuf),

    #[error("No tile sources found. Set sources by giving a database connection string on command line, env variable, or a config file.")]
    NoSources,

    #[error("Unrecognizable connection strings: {0:?}")]
    UnrecognizableConnections(Vec<String>),

    #[error(transparent)]
    PostgresError(#[from] PgError),

    #[error(transparent)]
    MbtilesError(#[from] MbtError),

    #[error(transparent)]
    FileError(#[from] FileError),

    #[cfg(feature = "sprites")]
    #[error(transparent)]
    SpriteError(#[from] SpriteError),

    #[cfg(feature = "fonts")]
    #[error(transparent)]
    FontError(#[from] FontError),

    #[error(transparent)]
    WebError(#[from] actix_web::Error),

    #[error("Internal error: {0}")]
    InternalError(Box<dyn Error>),
}
