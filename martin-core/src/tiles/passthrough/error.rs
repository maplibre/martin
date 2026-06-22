//! Error types for the `passthrough` HTTP upstream source.

/// Errors that can occur when proxying tiles from an upstream HTTP tile server.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum PassthroughError {
    /// A per-tile HTTP request to the upstream failed (network, connect, or timeout).
    #[error("Upstream request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// No upstream URL was configured for the source.
    #[error("Passthrough source {0} has no upstream URL configured")]
    EmptyUrlList(String),

    /// A configured request header name or value was not valid.
    #[error("Invalid request header: {0}")]
    InvalidHeader(String),

    /// A configured tile-URL template is missing one of the `{z}`/`{x}`/`{y}` placeholders.
    #[error("Tile URL template {0} must contain {{z}}, {{x}} and {{y}} placeholders")]
    InvalidUrlTemplate(String),

    /// Fetching the upstream `TileJSON` document failed.
    #[error("Failed to fetch TileJSON from {url}: {source}")]
    TileJsonFetch {
        /// The `TileJSON` URL that was requested.
        url: String,
        /// The underlying transport error.
        #[source]
        source: reqwest::Error,
    },

    /// The upstream returned a non-success status for the `TileJSON` document.
    #[error("Fetching TileJSON from {url} returned HTTP status {status}")]
    TileJsonStatus {
        /// The `TileJSON` URL that was requested.
        url: String,
        /// The HTTP status code returned by the upstream.
        status: u16,
    },

    /// The upstream `TileJSON` document could not be parsed.
    #[error("Failed to parse TileJSON from {url}: {source}")]
    TileJsonParse {
        /// The `TileJSON` URL that was requested.
        url: String,
        /// The underlying deserialization error.
        #[source]
        source: serde_json::Error,
    },

    /// The upstream `TileJSON` document contained no entries in its `tiles` array.
    #[error("TileJSON from {0} contained no tile URL templates")]
    NoTilesInTileJson(String),

    /// The tile format could not be derived from config, URL extension, or `TileJSON`.
    #[error(
        "Cannot determine tile format for passthrough source {0}; set `format` explicitly in the config"
    )]
    FormatUndeterminable(String),

    /// A per-tile request returned an unexpected (non-success, non-404/204) HTTP status.
    #[error("Upstream {url} returned unexpected HTTP status {status}")]
    UnexpectedStatus {
        /// The tile URL that was requested.
        url: String,
        /// The HTTP status code returned by the upstream.
        status: u16,
    },
}
