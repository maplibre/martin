use std::path::{Path, PathBuf};

#[cfg(feature = "fonts")]
use martin_core::fonts::FontError;
#[cfg(feature = "postgres")]
use martin_core::tiles::postgres::PostgresError;
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceCode};

pub type ConfigFileResult<T> = Result<T, ConfigFileError>;

#[derive(thiserror::Error, Debug)]
pub enum ConfigFileError {
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    #[error("Unable to load config file {1}: {0}")]
    ConfigLoadError(#[source] std::io::Error, PathBuf),

    #[error("Unable to parse YAML in config file {}: {}", .0.named_source.name(), .0.error)]
    YamlParseError(Box<YamlParseDetails>),

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
    PostgresPoolCreationFailed(#[source] PostgresError),

    #[cfg(feature = "fonts")]
    #[error("Failed to load fonts from {1}: {0}")]
    FontResolutionFailed(#[source] FontError, PathBuf),

    #[cfg(feature = "pmtiles")]
    #[error("Failed to parse object store URL of {1}: {0}")]
    ObjectStoreUrlParsing(object_store::Error, String),

    #[cfg(feature = "pmtiles")]
    #[error("Failed to list objects under {1}: {0}")]
    ObjectStoreList(object_store::Error, String),

    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Failed to start style render pool: {0}")]
    RendererPoolSpawnFailed(#[source] std::io::Error),
}

/// Boxed payload for [`ConfigFileError::YamlParseError`].
#[derive(Debug)]
pub struct YamlParseDetails {
    pub(crate) error: serde_saphyr::Error,
    pub(crate) named_source: NamedSource<String>,
}

impl ConfigFileError {
    /// Construct a YAML parse error with the originating source text and file path.
    ///
    /// The source text is retained so miette diagnostics can render the offending snippet.
    #[must_use]
    pub fn yaml_parse(error: serde_saphyr::Error, source_text: String, file_path: &Path) -> Self {
        Self::YamlParseError(Box::new(YamlParseDetails {
            error,
            named_source: NamedSource::new(file_path.display().to_string(), source_text),
        }))
    }

    /// Render this error as a [`miette::Report`] for graphical display, when applicable.
    #[must_use]
    pub fn to_miette_report(&self) -> Option<miette::Report> {
        match self {
            Self::YamlParseError(details) => {
                let inner = serde_saphyr::miette::to_miette_report(
                    &details.error,
                    details.named_source.inner(),
                    details.named_source.name(),
                );
                let kind = YamlReportKind::for_error(&details.error);
                Some(miette::Report::new(YamlParseReport { inner, kind }))
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum YamlReportKind {
    Substitution,
    Yaml,
}

impl YamlReportKind {
    fn for_error(err: &serde_saphyr::Error) -> Self {
        use serde_saphyr::Error::{UnresolvedProperty, InvalidPropertyName, PropertyRequiredButUnset, PropertyRequiredButEmpty, WithSnippet};

        match err {
            UnresolvedProperty { .. }
            | InvalidPropertyName { .. }
            | PropertyRequiredButUnset { .. }
            | PropertyRequiredButEmpty { .. } => Self::Substitution,
            WithSnippet { error, .. }
                if matches!(
                    error.as_ref(),
                    UnresolvedProperty { .. }
                        | InvalidPropertyName { .. }
                        | PropertyRequiredButUnset { .. }
                        | PropertyRequiredButEmpty { .. }
                ) =>
            {
                Self::Substitution
            }
            _ => Self::Yaml,
        }
    }

    fn code(self) -> &'static str {
        match self {
            Self::Substitution => "martin::config::substitution",
            Self::Yaml => "martin::config::yaml",
        }
    }

    fn help(self) -> &'static str {
        match self {
            Self::Substitution => {
                "Make sure every ${VAR} reference resolves to an environment variable, or supply a default with `${VAR:-fallback}`."
            }
            Self::Yaml => {
                "Check the highlighted token in your YAML. The error usually indicates a mismatched type or an unexpected shape."
            }
        }
    }
}

#[derive(Debug)]
struct YamlParseReport {
    inner: miette::Report,
    kind: YamlReportKind,
}

impl YamlParseReport {
    fn inner_diag(&self) -> &(dyn Diagnostic + 'static) {
        <miette::Report as AsRef<dyn Diagnostic>>::as_ref(&self.inner)
    }
}

impl std::fmt::Display for YamlParseReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self.inner_diag(), f)
    }
}

impl std::error::Error for YamlParseReport {}

impl Diagnostic for YamlParseReport {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(self.kind.code()))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(self.kind.help()))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("https://maplibre.org/martin/config-file/"))
    }

    fn severity(&self) -> Option<miette::Severity> {
        self.inner_diag().severity()
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        self.inner_diag().source_code()
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        self.inner_diag().labels()
    }

    fn related(&self) -> Option<Box<dyn Iterator<Item = &dyn Diagnostic> + '_>> {
        self.inner_diag().related()
    }

    fn diagnostic_source(&self) -> Option<&dyn Diagnostic> {
        self.inner_diag().diagnostic_source()
    }
}

impl Diagnostic for ConfigFileError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let code: &'static str = match self {
            Self::IoError(..) => "martin::config::io",
            Self::ConfigLoadError(..) => "martin::config::io::load",
            Self::ConfigWriteError(..) => "martin::config::io::write",
            Self::YamlParseError { .. } => "martin::config::yaml",
            Self::NoSources => "martin::config::no_sources",
            Self::InvalidFilePath(_) => "martin::config::invalid_file_path",
            Self::InvalidSourceUrl(..) => "martin::config::invalid_source_url",
            Self::PathNotConvertibleToUrl(_) => "martin::config::path_not_url",
            Self::InvalidSourceFilePath(..) => "martin::config::invalid_source_file_path",
            Self::CorsNoOriginsConfigured => "martin::config::cors::no_origins",
            #[cfg(feature = "styles")]
            Self::DirectoryWalking(..) => "martin::config::styles::walk",
            #[cfg(feature = "postgres")]
            Self::PostgresPoolSizeInvalid => "martin::config::postgres::pool_size",
            #[cfg(feature = "postgres")]
            Self::PostgresConnectionStringMissing => "martin::config::postgres::connection_string",
            #[cfg(feature = "postgres")]
            Self::PostgresPoolCreationFailed(_) => "martin::config::postgres::pool_creation",
            #[cfg(feature = "fonts")]
            Self::FontResolutionFailed(..) => "martin::config::fonts::resolution",
            #[cfg(feature = "pmtiles")]
            Self::ObjectStoreUrlParsing(..) => "martin::config::pmtiles::object_store_url",
            #[cfg(feature = "pmtiles")]
            Self::ObjectStoreList(..) => "martin::config::pmtiles::object_store_list",
            #[cfg(all(feature = "rendering", target_os = "linux"))]
            Self::RendererPoolSpawnFailed(_) => "martin::config::styles::render_pool_spawn",
        };
        Some(Box::new(code))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let help: &'static str = match self {
            Self::NoSources => {
                "Provide tile sources via --connection, environment variables (e.g. DATABASE_URL), or a config file passed with --config."
            }
            Self::CorsNoOriginsConfigured => {
                "Either set `cors: true` (allow all origins) or provide at least one entry in `origin` under the cors block."
            }
            Self::YamlParseError { .. } => {
                "Check the highlighted token in your YAML. The error usually indicates a mismatched type or an unexpected shape."
            }
            #[cfg(feature = "postgres")]
            Self::PostgresPoolSizeInvalid => {
                "Set `pool_size` to an integer greater than or equal to 1."
            }
            _ => return None,
        };
        Some(Box::new(help))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("https://maplibre.org/martin/config-file/"))
    }

    // Carets and labels come from `to_miette_report`.
    // Surface the file here so direct rendering still shows it.
    fn source_code(&self) -> Option<&dyn SourceCode> {
        match self {
            Self::YamlParseError(details) => Some(&details.named_source),
            _ => None,
        }
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        None
    }
}
