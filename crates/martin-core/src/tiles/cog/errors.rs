//! Error types for `Cloud Optimized GeoTIFF` operations.

/// Errors that can occur when working with COG files.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum CogError {
    /// Cannot open, read, or serve a COG.
    #[error(transparent)]
    Tiles(#[from] cog_tiles::CogError),

    /// The COG produces tiles in a format Martin cannot serve.
    #[error("{location} produces {format:?} tiles, which Martin cannot serve")]
    UnsupportedFormat {
        /// Display form of the source.
        location: String,
        /// The tile format reported by the COG reader.
        format: cog_tiles::TileFormat,
    },

    /// Encoding a tile on the blocking thread pool panics or is cancelled.
    #[error("Unable to encode a tile: {join}")]
    Encode {
        /// Why the encoding task did not finish.
        #[source]
        join: tokio::task::JoinError,
    },
}

impl crate::Classify for CogError {
    fn kind(&self) -> crate::ErrorKind {
        crate::ErrorKind::Internal
    }
}
