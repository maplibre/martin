use std::path::PathBuf;

#[cfg(feature = "fonts")]
use martin_core::fonts::FontError;
#[cfg(feature = "postgres")]
use martin_core::tiles::postgres::PostgresError;
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceCode, SourceSpan};

pub type ConfigFileResult<T> = Result<T, ConfigFileError>;

#[derive(thiserror::Error, Debug)]
pub enum ConfigFileError {
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    #[error("Unable to load config file {1}: {0}")]
    ConfigLoadError(#[source] std::io::Error, PathBuf),

    #[error("Unable to parse YAML in config file {}: {}", .0.file_path.display(), .0.error)]
    YamlParseError(Box<YamlParseDetails>),

    #[error("Unable to substitute environment variables in config file {}: {}", .0.file_path.display(), .0.source)]
    SubstitutionError(Box<SubstitutionDetails>),

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
}

/// Boxed payload for [`ConfigFileError::YamlParseError`].
#[derive(Debug)]
pub struct YamlParseDetails {
    pub error: serde_saphyr::Error,
    pub named_source: NamedSource<String>,
    pub file_path: PathBuf,
}

/// Boxed payload for [`ConfigFileError::SubstitutionError`].
#[derive(Debug)]
pub struct SubstitutionDetails {
    pub source: subst::Error,
    pub named_source: NamedSource<String>,
    pub primary_span: Option<SourceSpan>,
    pub file_path: PathBuf,
}

impl ConfigFileError {
    /// Construct a YAML parse error with the originating source text and file path.
    ///
    /// The source text is retained so miette diagnostics can render the offending snippet.
    #[must_use]
    pub fn yaml_parse(
        error: serde_saphyr::Error,
        source_text: String,
        file_path: PathBuf,
    ) -> Self {
        let display_name = file_path.display().to_string();
        Self::YamlParseError(Box::new(YamlParseDetails {
            error,
            named_source: NamedSource::new(display_name, source_text),
            file_path,
        }))
    }

    /// Construct a substitution error, locating the failing variable token within the source.
    #[must_use]
    pub fn substitution(
        source: subst::Error,
        source_text: String,
        file_path: PathBuf,
    ) -> Self {
        let primary_span = subst_error_span(&source, &source_text);
        let display_name = file_path.display().to_string();
        Self::SubstitutionError(Box::new(SubstitutionDetails {
            source,
            named_source: NamedSource::new(display_name, source_text),
            primary_span,
            file_path,
        }))
    }

    /// Render this error as a [`miette::Report`] for graphical display, when applicable.
    ///
    /// Returns `Some(_)` for spanned errors (YAML parse and substitution failures) where a
    /// graphical snippet is more useful than plain text. Returns `None` for errors that
    /// don't carry source location information; callers should fall back to [`Display`].
    #[must_use]
    pub fn to_miette_report(&self) -> Option<miette::Report> {
        match self {
            Self::YamlParseError(details) => {
                let file = details.file_path.display().to_string();
                Some(serde_saphyr::miette::to_miette_report(
                    &details.error,
                    details.named_source.inner(),
                    &file,
                ))
            }
            Self::SubstitutionError(_) => Some(miette::Report::new(
                SubstitutionDiagnostic::from_error(self),
            )),
            _ => None,
        }
    }
}

/// Self-contained [`miette::Diagnostic`] for a substitution error, owning its source text
/// so it can produce a `'static` [`miette::Report`].
#[derive(Debug)]
struct SubstitutionDiagnostic {
    message: String,
    help: &'static str,
    named_source: NamedSource<String>,
    primary_span: Option<SourceSpan>,
    label_text: String,
}

impl SubstitutionDiagnostic {
    fn from_error(err: &ConfigFileError) -> Self {
        let ConfigFileError::SubstitutionError(details) = err else {
            unreachable!("SubstitutionDiagnostic::from_error called on non-substitution error")
        };
        Self {
            message: format!("{err}"),
            help: "Make sure every ${VAR} reference resolves to an environment variable, or supply a default with `${VAR:-fallback}`.",
            named_source: NamedSource::new(
                details.named_source.name(),
                details.named_source.inner().clone(),
            ),
            primary_span: details.primary_span,
            label_text: details.source.to_string(),
        }
    }
}

impl std::fmt::Display for SubstitutionDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SubstitutionDiagnostic {}

impl Diagnostic for SubstitutionDiagnostic {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("martin::config::substitution"))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(self.help))
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        Some(&self.named_source)
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let span = self.primary_span?;
        let label = LabeledSpan::new_primary_with_span(Some(self.label_text.clone()), span);
        Some(Box::new(std::iter::once(label)))
    }
}

impl Diagnostic for ConfigFileError {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        let code: &'static str = match self {
            Self::IoError(..) => "martin::config::io",
            Self::ConfigLoadError(..) => "martin::config::io::load",
            Self::ConfigWriteError(..) => "martin::config::io::write",
            Self::YamlParseError { .. } => "martin::config::yaml",
            Self::SubstitutionError { .. } => "martin::config::substitution",
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
            Self::SubstitutionError { .. } => {
                "Make sure every ${VAR} reference resolves to an environment variable, or supply a default with `${VAR:-fallback}`."
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

    fn source_code(&self) -> Option<&dyn SourceCode> {
        match self {
            Self::YamlParseError(details) => Some(&details.named_source),
            Self::SubstitutionError(details) => Some(&details.named_source),
            _ => None,
        }
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        match self {
            Self::YamlParseError(details) => {
                let location = details.error.location()?;
                let span = location.span();
                let offset = usize::try_from(span.byte_offset()?).ok()?;
                let raw_len = usize::try_from(span.byte_len().unwrap_or(1)).ok()?;
                // Clamp to source length so a one-past-end span (often emitted at EOF) still lands on
                // a visible character.
                let source_len = details.named_source.inner().len();
                let length = if offset >= source_len {
                    0
                } else {
                    raw_len.min(source_len - offset).max(1)
                };
                let label = LabeledSpan::new_primary_with_span(
                    Some(details.error.to_string()),
                    SourceSpan::new(offset.into(), length),
                );
                Some(Box::new(std::iter::once(label)))
            }
            Self::SubstitutionError(details) => {
                let span = details.primary_span?;
                let label = LabeledSpan::new_primary_with_span(
                    Some(details.source.to_string()),
                    span,
                );
                Some(Box::new(std::iter::once(label)))
            }
            _ => None,
        }
    }
}

/// Locate the failing token in `source_text` that corresponds to a substitution failure.
fn subst_error_span(error: &subst::Error, source_text: &str) -> Option<SourceSpan> {
    let range = error.source_range();
    if range.start >= source_text.len() {
        return None;
    }
    let length = range.len().max(1).min(source_text.len() - range.start);
    Some(SourceSpan::new(range.start.into(), length))
}
