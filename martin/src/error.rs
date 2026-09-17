//! The error `main` sees.
//!
//! [`StartupError`] is the union of the *phases* that can abort a launch, not of everything
//! that can go wrong in the crate. Each variant wraps one seam's error, and those seams have
//! consumers that never need this type - notably [`SourceBuildError`], which hot reload logs
//! and discards. Runtime tile serving does not appear here at all; it ends at an HTTP
//! response via [`MartinCoreError`](martin_core::tiles::MartinCoreError).

use std::io;

use crate::config::args::ArgsError;
use crate::config::file::ConfigFileError;
#[cfg(feature = "_tiles")]
use crate::config::file::SourceBuildError;
use crate::srv::ServerStartError;

/// A convenience [`Result`] for fallible startup steps.
pub type StartupResult<T> = Result<T, StartupError>;

/// Why martin could not finish starting up.
#[derive(thiserror::Error, Debug)]
pub enum StartupError {
    #[error(transparent)]
    Args(#[from] ArgsError),

    #[error(transparent)]
    Config(#[from] ConfigFileError),

    #[cfg(feature = "_tiles")]
    #[error(transparent)]
    SourceBuild(#[from] SourceBuildError),

    #[error(transparent)]
    Server(#[from] ServerStartError),

    #[error(transparent)]
    Io(#[from] io::Error),
}

impl StartupError {
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
    /// * [`LogFormat::Json`](crate::logging::LogFormat::Json) ->
    ///   [`miette::JSONReportHandler`], emitting a structured JSON object with `message`,
    ///   `severity`, `code`, `help`, `url`, `filename`, and a `labels` array. Suitable for
    ///   editor tooling, CI, and log aggregation that already consumes the rest of
    ///   `martin`'s output as JSON.
    /// * Any other format -> [`miette::GraphicalReportHandler`], i.e. the snippet/caret
    ///   output a human reads on the terminal.
    ///
    /// Errors that don't carry source location info fall back to plain [`Display`] in both
    /// modes (or a one-line JSON object in JSON mode), since there's nothing for miette to
    /// render against.
    #[must_use]
    pub fn render_diagnostic_with(&self, format: crate::logging::LogFormat) -> String {
        if let Some(report) = self
            .spanned_config_error()
            .and_then(ConfigFileError::to_miette_report)
        {
            if format.is_json() {
                let mut buf = String::new();
                miette::JSONReportHandler::new()
                    .render_report(&mut buf, report.as_ref())
                    .expect("rendering into a String is infallible");
                return buf;
            }
            return format!("{report:?}");
        }
        if format.is_json() {
            // Best-effort JSON envelope so machine consumers always receive a JSON document
            // even for non-spanned errors. `serde_json::to_string` on a `String` is
            // infallible - strings are always valid JSON.
            let message = serde_json::to_string(&self.to_string())
                .expect("string serialization is infallible");
            return format!(r#"{{"message": {message}}}"#);
        }
        format!("{self}")
    }

    /// The config error carrying source spans, if this failure bottoms out in one.
    ///
    /// A config problem can reach `main` directly or via a source build that rejected the
    /// config it was handed; both should render the same caret diagnostic.
    const fn spanned_config_error(&self) -> Option<&ConfigFileError> {
        match self {
            Self::Config(e) => Some(e),
            #[cfg(feature = "_tiles")]
            Self::SourceBuild(SourceBuildError::Config(e)) => Some(e),
            _ => None,
        }
    }
}
