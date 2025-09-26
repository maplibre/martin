//! Error types for style processing operations.

/// Errors that can occur during style processing.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum StyleError {
    /// Rendering is disabled
    #[error(
        "Rendering is disabled. Please see styles.experimental_rendering for further information."
    )]
    RenderingIsDisabled,

    /// IO error
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, std::path::PathBuf),

    /// Cannot render style
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error(transparent)]
    RenderingPoolError(#[from] maplibre_native::SingleThreadedRenderPoolError),
}
