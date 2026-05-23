#[cfg(all(feature = "rendering", target_os = "linux"))]
use maplibre_native::RenderingError;

/// Errors that can occur during style processing.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum StyleError {
    /// I/O error while loading a style for rendering.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    /// `maplibre-native` failed to produce a frame.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error(transparent)]
    RenderingError(#[from] RenderingError),

    /// Render request never reached the worker (channel closed).
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Failed to send request to rendering thread")]
    FailedToSendRequest,

    /// Worker dropped the response channel before answering.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Failed to receive response from rendering thread")]
    FailedToReceiveResponse,

    /// Rendering is disabled by configuration.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Rendering is disabled")]
    RenderingIsDisabled,
}
