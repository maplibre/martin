use std::path::PathBuf;

pub type ConfigFileResult<T> = Result<T, ConfigFileError>;

#[derive(thiserror::Error, Debug)]
pub enum ConfigFileError {
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    #[error("Unable to load config file {1}: {0}")]
    ConfigLoadError(#[source] std::io::Error, PathBuf),

    #[error("Unable to parse config file {1}: {0}")]
    ConfigParseError(#[source] subst::yaml::Error, PathBuf),

    #[error("Unable to write config file {1}: {0}")]
    ConfigWriteError(#[source] std::io::Error, PathBuf),

    #[error(
        "No tile sources found. Set sources by giving a database connection string on command line, env variable, or a config file."
    )]
    NoSources,
    #[error("Source path is not a file: {0}")]
    InvalidFilePath(PathBuf),

    #[error("Error {0} while parsing URL {1}")]
    InvalidSourceUrl(#[source] url::ParseError, String),

    #[error("Could not parse source path {0} as a URL")]
    PathNotConvertibleToUrl(PathBuf),

    #[error("Source {0} uses bad file {1}")]
    InvalidSourceFilePath(String, PathBuf),

    #[error("At least one 'origin' must be specified in the 'cors' configuration")]
    CorsNoOriginsConfigured,

    #[cfg(feature = "styles")]
    #[error("Walk directory error {0}: {1}")]
    DirectoryWalking(#[source] walkdir::Error, PathBuf),

    #[cfg(feature = "postgres")]
    #[error("The postgres pool_size must be greater than or equal to 1")]
    PostgresPoolSizeInvalid,

    #[cfg(feature = "postgres")]
    #[error("A postgres connection string must be provided")]
    PostgresConnectionStringMissing,

    #[cfg(feature = "postgres")]
    #[error("Failed to create postgres pool: {0}")]
    PostgresPoolCreationFailed(#[source] martin_core::tiles::postgres::PostgresError),

    #[cfg(feature = "fonts")]
    #[error("Failed to load fonts from {1}: {0}")]
    FontResolutionFailed(#[source] martin_core::fonts::FontError, PathBuf),

    #[cfg(feature = "pmtiles")]
    #[error("Failed to parse object store URL of {1}: {0}")]
    ObjectStoreUrlParsing(object_store::Error, String),
}
