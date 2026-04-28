use std::fmt::Write as _;
use std::io;

#[cfg(feature = "unstable-cog")]
use martin_core::tiles::cog::CogError;
#[cfg(feature = "mbtiles")]
use martin_core::tiles::mbtiles::MbtilesError;
#[cfg(feature = "pmtiles")]
use martin_core::tiles::pmtiles::PmtilesError;
#[cfg(feature = "postgres")]
use martin_core::tiles::postgres::PostgresError;

use crate::config::file::ConfigFileError;

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
    PostgresError(#[from] PostgresError),

    #[cfg(feature = "pmtiles")]
    #[error(transparent)]
    PmtilesError(#[from] PmtilesError),

    #[cfg(feature = "mbtiles")]
    #[error(transparent)]
    MbtilesError(#[from] MbtilesError),

    #[cfg(feature = "unstable-cog")]
    #[error(transparent)]
    CogError(#[from] CogError),

    #[error(transparent)]
    ConfigFileError(#[from] ConfigFileError),

    #[cfg(feature = "sprites")]
    #[error(transparent)]
    SpriteError(#[from] martin_core::sprites::SpriteError),

    #[error(transparent)]
    IoError(#[from] io::Error),

    #[cfg(feature = "lambda")]
    #[error(transparent)]
    LambdaError(#[from] lambda_web::LambdaError),

    #[cfg(feature = "metrics")]
    #[error("could not initialize metrics: {0}")]
    MetricsIntialisationError(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("warnings issued during tile source resolution")]
    TileResolutionWarningsIssued,

    #[error("could not create a watcher for directories configured for tile source discovery")]
    DirectoryWatchError(notify::ErrorKind),

    #[error("Source '{0}' not found in discovered sources")]
    SourceNotFound(String),
}

impl MartinError {
    /// Format the error for end-user display using miette's graphical reporter.
    ///
    /// See [`render_diagnostic_with`](Self::render_diagnostic_with) for an explanation of
    /// the rendering choice. This is a shorthand for the graphical case.
    #[must_use]
    pub fn render_diagnostic(&self) -> String {
        self.render_diagnostic_with(crate::logging::LogFormat::default())
    }

    /// Format the error for end-user display using a chosen output format.
    ///
    /// Configuration errors that carry source spans (YAML parse errors and substitution
    /// failures) are rendered through miette so the user sees a pointer into the offending
    /// file. The `format` controls *how* miette renders:
    ///
    /// * [`LogFormat::Json`](crate::logging::LogFormat::Json) →
    ///   [`miette::JSONReportHandler`], emitting a structured JSON object with `message`,
    ///   `severity`, `code`, `help`, `url`, `filename`, and a `labels` array. Suitable for
    ///   editor tooling, CI, and log aggregation that already consumes the rest of
    ///   `martin`'s output as JSON.
    /// * Any other format → [`miette::GraphicalReportHandler`], i.e. the snippet/caret
    ///   output a human reads on the terminal.
    ///
    /// Errors that don't carry source location info fall back to plain [`Display`] in both
    /// modes (or a one-line JSON object in JSON mode), since there's nothing for miette to
    /// render against.
    #[must_use]
    pub fn render_diagnostic_with(&self, format: crate::logging::LogFormat) -> String {
        if let MartinError::ConfigFileError(cfg_err) = self
            && let Some(report) = cfg_err.to_miette_report()
        {
            return if format.is_json() {
                let mut buf = String::new();
                let handler = miette::JSONReportHandler::new();
                // `JSONReportHandler::render_report` only fails when the underlying
                // `Write` impl fails, and `String::write_*` is infallible — but treat any
                // unexpected error defensively by falling through to Display.
                if handler.render_report(&mut buf, report.as_ref()).is_ok() {
                    buf
                } else {
                    format!("{self}")
                }
            } else {
                format!("{report:?}")
            };
        }
        if format.is_json() {
            // Best-effort JSON envelope so machine consumers always receive a JSON document
            // even for non-spanned errors.
            let message =
                serde_json::to_string(&self.to_string()).unwrap_or_else(|_| "\"\"".to_string());
            return format!(r#"{{"message": {message}}}"#);
        }
        format!("{self}")
    }
}
