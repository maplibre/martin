#[cfg(all(feature = "rendering", target_os = "linux"))]
use std::time::Duration;

#[cfg(all(feature = "rendering", target_os = "linux"))]
use crate::overlay::ApplyError;

/// Errors that can occur during style processing.
#[derive(thiserror::Error, Debug)]
pub enum StyleError {
    /// I/O error while loading a style for rendering.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    /// `MapLibre` failed to load the style.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Failed to load style: {message}")]
    StyleLoadError {
        /// `MapLibre`'s reason.
        message: String,
    },

    /// `MapLibre` failed to produce a frame.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error(transparent)]
    RenderingError(#[from] maplibre_native_ffi::Error),

    /// `MapLibre` did not answer a call within the time it is given.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("MapLibre did not answer within {timeout:?}: {call}")]
    Timeout {
        /// The call that went unanswered.
        call: &'static str,
        /// How long it was waited for.
        timeout: Duration,
    },

    /// No graphics device can be created, so no renderer can be created.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("No graphics device can render styles: {reason}")]
    GraphicsUnavailable {
        /// `MapLibre`'s reason.
        reason: String,
    },

    /// Overlay application against the renderer's style failed.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error(transparent)]
    OverlayApply(#[from] ApplyError),

    /// Render request never reached the worker (channel closed).
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Failed to send request to rendering thread")]
    FailedToSendRequest,

    /// Worker dropped the response channel before answering.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Failed to receive response from rendering thread")]
    FailedToReceiveResponse,

    /// The renderer panicked while handling the request.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Rendering thread panicked while handling the request")]
    RenderingPanicked,

    /// Rendering is disabled by configuration.
    #[cfg(all(feature = "rendering", target_os = "linux"))]
    #[error("Rendering is disabled")]
    RenderingIsDisabled,
}
