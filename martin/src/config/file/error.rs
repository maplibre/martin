use std::path::{Path, PathBuf};

#[cfg(feature = "fonts")]
use martin_core::fonts::FontError;
#[cfg(feature = "postgres")]
use martin_core::tiles::postgres::{PostgresError, redact_conn_str};
use miette::{Diagnostic, LabeledSpan, NamedSource, SourceCode, SourceSpan};

pub type ConfigFileResult<T> = Result<T, ConfigFileError>;

#[derive(thiserror::Error, Debug)]
pub enum ConfigFileError {
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),

    #[error("Unable to load config file {1}: {0}")]
    ConfigLoadError(#[source] std::io::Error, PathBuf),

    #[error("Unable to parse YAML in config file {}: {}", .0.named_source.name(), .0.error)]
    YamlParseError(Box<YamlParseDetails>),

    #[error("Unable to substitute environment variables in config file {}: {}", .0.named_source.name(), .0.source)]
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
    #[error("Failed to create postgres pool: {}", .0.source)]
    PostgresPoolCreationFailed(Box<PostgresPoolDetails>),

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

/// Boxed payload for [`ConfigFileError::SubstitutionError`].
#[derive(Debug)]
pub struct SubstitutionDetails {
    pub(crate) source: subst::Error,
    pub(crate) named_source: NamedSource<String>,
    pub(crate) primary_span: Option<SourceSpan>,
}

/// Boxed payload for [`ConfigFileError::PostgresPoolCreationFailed`].
///
/// `named_source`/`primary_span` are populated lazily (see
/// [`ConfigFileError::with_conn_str_span`]) once the config-file source is available, so the
/// diagnostic can point at the offending `connection_string` line. They stay `None` when the
/// sources came from the CLI rather than a file, in which case the error renders as plain text.
#[cfg(feature = "postgres")]
#[derive(Debug)]
pub struct PostgresPoolDetails {
    pub(crate) source: PostgresError,
    pub(crate) named_source: Option<NamedSource<String>>,
    pub(crate) primary_span: Option<SourceSpan>,
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

    /// Construct a postgres pool-creation error without source-location info.
    ///
    /// The connection-string span is attached later via [`Self::with_conn_str_span`], once the
    /// config-file source text is in scope.
    #[cfg(feature = "postgres")]
    #[must_use]
    pub fn postgres_pool_creation(source: PostgresError) -> Self {
        Self::PostgresPoolCreationFailed(Box::new(PostgresPoolDetails {
            source,
            named_source: None,
            primary_span: None,
        }))
    }

    /// Attach the offending `connection_string`'s location within the config file to a
    /// [`Self::PostgresPoolCreationFailed`] error, so it can be rendered as a miette diagnostic
    /// pointing at the right line.
    ///
    /// `conn_str` is the raw (unredacted) connection string; `source` is the config-file source
    /// **with passwords already redacted**. We redact `conn_str` the same way before locating it,
    /// so the search succeeds and no secret is compared or stored. Any other error variant, a
    /// missing source, or a connection string we can't locate is returned unchanged.
    #[cfg(feature = "postgres")]
    #[must_use]
    pub(crate) fn with_conn_str_span(
        mut self,
        conn_str: Option<&str>,
        source: Option<&NamedSource<String>>,
    ) -> Self {
        if let Self::PostgresPoolCreationFailed(details) = &mut self
            && let (Some(conn), Some(named_source)) = (conn_str, source)
        {
            let redacted = redact_conn_str(conn);
            if let Some(span) = locate_span(named_source.inner(), &redacted) {
                details.named_source = Some(named_source.clone());
                details.primary_span = Some(span);
            }
        }
        self
    }

    /// Construct a substitution error, locating the failing variable token within the source.
    #[must_use]
    pub fn substitution(source: subst::Error, source_text: String, file_path: &Path) -> Self {
        let primary_span = subst_error_span(&source, &source_text);
        Self::SubstitutionError(Box::new(SubstitutionDetails {
            source,
            named_source: NamedSource::new(file_path.display().to_string(), source_text),
            primary_span,
        }))
    }

    /// Render this error as a [`miette::Report`] for graphical display, when applicable.
    ///
    /// For YAML parse errors we delegate to `serde_saphyr::miette::to_miette_report`, which
    /// builds a richer diagnostic (snippet windows, nested labels) than our manual
    /// `Diagnostic` impl below. The substitution path uses an owned [`SubstitutionReport`]
    /// because `miette::Report::new` requires `'static` data and `subst::Error` isn't
    /// `Clone`, so we can't put `self` inside the report directly.
    #[must_use]
    pub fn to_miette_report(&self) -> Option<miette::Report> {
        match self {
            Self::YamlParseError(details) => Some(serde_saphyr::miette::to_miette_report(
                &details.error,
                details.named_source.inner(),
                details.named_source.name(),
            )),
            Self::SubstitutionError(details) => Some(miette::Report::new(SubstitutionReport {
                message: format!("{self}"),
                named_source: NamedSource::new(
                    details.named_source.name(),
                    details.named_source.inner().clone(),
                ),
                primary_span: details.primary_span,
                label_text: details.source.to_string(),
            })),
            #[cfg(feature = "postgres")]
            Self::PostgresPoolCreationFailed(details) => {
                // Only renderable once a source span was attached (see `with_conn_str_span`);
                // otherwise fall back to plain `Display`.
                let named_source = details.named_source.as_ref()?;
                Some(miette::Report::new(PostgresPoolReport {
                    message: format!("{self}"),
                    named_source: NamedSource::new(
                        named_source.name(),
                        named_source.inner().clone(),
                    ),
                    primary_span: details.primary_span,
                    label_text: details.source.to_string(),
                }))
            }
            _ => None,
        }
    }
}

/// Self-contained `Diagnostic` for a substitution error, owning all its data so it can
/// power a `'static miette::Report` without having to make `ConfigFileError: Clone`.
#[derive(Debug)]
struct SubstitutionReport {
    message: String,
    named_source: NamedSource<String>,
    primary_span: Option<SourceSpan>,
    label_text: String,
}

impl std::fmt::Display for SubstitutionReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SubstitutionReport {}

impl Diagnostic for SubstitutionReport {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("martin::config::substitution"))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(
            "Make sure every ${VAR} reference resolves to an environment variable, or supply a default with `${VAR:-fallback}`.",
        ))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("https://maplibre.org/martin/config-file/"))
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

/// Self-contained `Diagnostic` for a postgres pool-creation failure, owning all its data (with
/// passwords already redacted) so it can power a `'static miette::Report`. Mirrors
/// [`SubstitutionReport`]; see [`ConfigFileError::to_miette_report`].
#[cfg(feature = "postgres")]
#[derive(Debug)]
struct PostgresPoolReport {
    message: String,
    named_source: NamedSource<String>,
    primary_span: Option<SourceSpan>,
    label_text: String,
}

#[cfg(feature = "postgres")]
impl std::fmt::Display for PostgresPoolReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

#[cfg(feature = "postgres")]
impl std::error::Error for PostgresPoolReport {}

#[cfg(feature = "postgres")]
impl Diagnostic for PostgresPoolReport {
    fn code<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("martin::config::postgres::pool_creation"))
    }

    fn help<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new(
            "Check the highlighted connection string. The username, host, port, and database name are shown; only the password is hidden.",
        ))
    }

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("https://maplibre.org/martin/config-file/"))
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

/// Locate `needle` within `haystack`, returning its byte span for a miette label.
///
/// Used to point at a (password-redacted) connection string inside the config-file source.
/// Returns `None` for an empty needle or when the text isn't found (e.g. unusual YAML quoting),
/// in which case the caller renders the error without a snippet.
#[cfg(feature = "postgres")]
fn locate_span(haystack: &str, needle: &str) -> Option<SourceSpan> {
    if needle.is_empty() {
        return None;
    }
    let offset = haystack.find(needle)?;
    Some(SourceSpan::new(offset.into(), needle.len()))
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

    fn url<'a>(&'a self) -> Option<Box<dyn std::fmt::Display + 'a>> {
        Some(Box::new("https://maplibre.org/martin/config-file/"))
    }

    fn source_code(&self) -> Option<&dyn SourceCode> {
        // `YamlParseError` is rendered through `serde_saphyr::miette::to_miette_report` in
        // `to_miette_report`, which carries its own source/labels - we only surface
        // `source_code` for the substitution path so direct consumers of the `Diagnostic`
        // trait still get useful output.
        match self {
            Self::SubstitutionError(details) => Some(&details.named_source),
            _ => None,
        }
    }

    fn labels(&self) -> Option<Box<dyn Iterator<Item = LabeledSpan> + '_>> {
        let Self::SubstitutionError(details) = self else {
            return None;
        };
        let span = details.primary_span?;
        let label = LabeledSpan::new_primary_with_span(Some(details.source.to_string()), span);
        Some(Box::new(std::iter::once(label)))
    }
}

/// Locate the failing token in `source_text` that corresponds to a substitution failure.
fn subst_error_span(error: &subst::Error, source_text: &str) -> Option<SourceSpan> {
    let range = error.source_range();
    (range.start < source_text.len()).then(|| SourceSpan::new(range.start.into(), range.len()))
}
