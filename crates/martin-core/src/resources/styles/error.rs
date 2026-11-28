#[cfg(feature = "rendering")]
use std::time::Duration;

#[cfg(feature = "rendering")]
use crate::overlay::ApplyError;
#[cfg(feature = "rendering")]
use crate::resources::styles::renderer::EngineError;

/// Errors that can occur during style processing.
#[derive(thiserror::Error, Debug)]
pub enum StyleError {
    /// I/O error while loading a style for rendering.
    #[cfg(feature = "rendering")]
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    /// `MapLibre` failed to load the style.
    #[cfg(feature = "rendering")]
    #[error("Failed to load style: {message}")]
    StyleLoadError {
        /// `MapLibre`'s reason.
        message: String,
    },

    /// `MapLibre` failed to produce a frame.
    #[cfg(feature = "rendering")]
    #[error(transparent)]
    RenderingError(#[from] EngineError),

    /// `MapLibre` did not answer a call within the time it is given.
    #[cfg(feature = "rendering")]
    #[error("MapLibre did not answer within {timeout:?}: {call}")]
    Timeout {
        /// The call that went unanswered.
        call: &'static str,
        /// How long it was waited for.
        timeout: Duration,
    },

    /// No graphics device can be created, so no renderer can be created.
    #[cfg(feature = "rendering")]
    #[error("No graphics device can render styles: {reason}")]
    GraphicsUnavailable {
        /// `MapLibre`'s reason.
        reason: String,
    },

    /// Overlay application against the renderer's style failed.
    #[cfg(feature = "rendering")]
    #[error(transparent)]
    OverlayApply(#[from] ApplyError),

    /// Render request never reached the worker (channel closed).
    #[cfg(feature = "rendering")]
    #[error("Failed to send request to rendering thread")]
    FailedToSendRequest,

    /// Worker dropped the response channel before answering.
    #[cfg(feature = "rendering")]
    #[error("Failed to receive response from rendering thread")]
    FailedToReceiveResponse,

    /// The renderer panicked while handling the request.
    #[cfg(feature = "rendering")]
    #[error("Rendering thread panicked while handling the request")]
    RenderingPanicked,

    /// Rendering is disabled by configuration.
    #[cfg(feature = "rendering")]
    #[error("Rendering is disabled")]
    RenderingIsDisabled,
}
