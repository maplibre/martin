/// Errors produced while applying an [`OverlaySpec`](crate::overlay::OverlaySpec)
/// to a renderer's map.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// Maplibre rejected a source or layer mutation.
    #[error("overlay {id:?}: maplibre rejected style mutation: {source}")]
    Maplibre {
        /// Synthetic id that triggered the error (un-prefixed, e.g. `f0-fill`).
        id: String,
        /// Underlying maplibre error.
        #[source]
        source: maplibre_native_ffi::Error,
    },
}
