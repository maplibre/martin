/// Errors that can occur during style processing.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum StyleError {
    /// Cannot render style.
    #[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
    #[error("Sprite {0} not found")]
    SingleThreadedRenderPoolError(#[from] maplibre_native::SingleThreadedRenderPoolError),

    /// Rendering is disabled.
    #[cfg(all(feature = "unstable-rendering", target_os = "linux"))]
    #[error("Rendering is disabled")]
    RenderingIsDisabled,
}
