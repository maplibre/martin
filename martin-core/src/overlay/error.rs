use maplibre_native::{GeoJsonError, StyleError};

/// Errors produced while applying an [`OverlaySpec`](crate::overlay::OverlaySpec)
/// to a maplibre [`StyleRef`](maplibre_native::StyleRef).
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// Failed to hand a feature's `GeoJSON` data to maplibre.
    #[error("overlay feature {index}: failed to convert GeoJSON: {source}")]
    GeoJsonConvert {
        /// Zero-based index of the offending feature.
        index: usize,
        /// Underlying maplibre error.
        #[source]
        source: GeoJsonError,
    },

    /// Maplibre rejected a source or layer mutation.
    #[error("overlay {id:?}: maplibre rejected style mutation: {source}")]
    Maplibre {
        /// Synthetic id that triggered the error (un-prefixed, e.g. `f0-fill`).
        id: String,
        /// Underlying maplibre error.
        #[source]
        source: StyleError,
    },
}
