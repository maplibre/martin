/// Error produced when maplibre rejects a source or layer while applying an
/// [`OverlaySpec`](crate::overlay::OverlaySpec) to a renderer's map.
#[derive(Debug, thiserror::Error)]
#[error("overlay {id:?}: maplibre rejected style mutation: {error}")]
pub struct ApplyError {
    /// Synthetic id that triggered the error (un-prefixed, e.g. `f0-fill`).
    pub id: String,
    /// Underlying maplibre error.
    #[source]
    pub error: maplibre_native_ffi::Error,
}
