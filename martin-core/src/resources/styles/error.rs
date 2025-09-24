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
}
