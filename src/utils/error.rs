use crate::file_config::FileError;
use crate::pg::PgError;
use std::io;
use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("The --config and the connection parameters cannot be used together")]
    ConfigAndConnectionsError,

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

    #[error("{0}")]
    FileError(#[from] FileError),

    #[error("{0}")]
    PostgresError(#[from] PgError),
}
