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

    /// A request to a STAC API fails.
    #[error("Unable to reach the STAC API at {url}: {source}")]
    StacRequest {
        /// The URL requested.
        url: String,
        /// Why the request failed.
        source: reqwest::Error,
    },

    /// A STAC API answers with an error status.
    #[error("The STAC API at {url} answered {status}: {body}")]
    StacStatus {
        /// The URL requested.
        url: String,
        /// The HTTP status code.
        status: u16,
        /// The start of the response body.
        body: String,
    },

    /// A STAC API answers with something other than the expected JSON.
    #[error("Unable to read the STAC API answer from {url}: {source}")]
    StacResponse {
        /// The URL requested.
        url: String,
        /// Why the answer cannot be read.
        source: serde_json::Error,
    },

    /// A STAC collection has no item matching the filters.
    #[error("No item of STAC collection {collection} matches the filters at {url}")]
    StacEmpty {
        /// The search URL.
        url: String,
        /// The collection searched.
        collection: String,
    },

    /// A STAC item found first has no usable bbox or asset.
    #[error("STAC item {item} has no bbox, no {asset} asset, or an asset href that cannot be read")]
    StacUnusableItem {
        /// The item ID.
        item: String,
        /// The asset key.
        asset: String,
    },

    /// A selected band is not one of the asset's bands.
    #[error("Band {band} is not a band of the {asset} asset: {available}")]
    StacBand {
        /// The band as configured.
        band: String,
        /// The asset key.
        asset: String,
        /// The bands the asset has, or why none can be matched.
        available: String,
    },

    /// A search page links its next page outside the STAC API's origin, which is not followed.
    #[error("The STAC API at {api} links its next search page to {link}, outside its origin")]
    StacForeignLink {
        /// The configured STAC API.
        api: String,
        /// The next page's URL.
        link: String,
    },

    /// The lowest zoom served is below the zoom each search covers a tile of.
    #[error("The min zoom {min_zoom} is below the search zoom {search_zoom}")]
    StacMinZoom {
        /// The lowest zoom served.
        min_zoom: u8,
        /// The zoom of the tile each search covers.
        search_zoom: u8,
    },

    /// A tile index cannot be read.
    #[error("Cannot read the tile index {}: {error}", path.display())]
    TileIndexRead {
        /// The index file.
        path: std::path::PathBuf,
        /// Why it cannot be read.
        #[source]
        error: sqlx::Error,
    },

    /// A tile index has no single feature table to read, and no `layer` is set.
    #[error("The tile index {} has no single feature table, set `layer` to one of: {layers}", path.display())]
    TileIndexLayer {
        /// The index file.
        path: std::path::PathBuf,
        /// The feature tables it has.
        layers: String,
    },

    /// A tile index has no feature table of the configured name.
    #[error("The tile index {} has no feature table {layer}", path.display())]
    TileIndexNoLayer {
        /// The index file.
        path: std::path::PathBuf,
        /// The configured layer.
        layer: String,
    },

    /// The footprints of a tile index are not in `EPSG:4326`.
    #[error(
        "The footprints of {layer} in the tile index {} are in {srs}, not EPSG:4326; build it with `gdaltindex -t_srs EPSG:4326`",
        path.display()
    )]
    TileIndexSrs {
        /// The index file.
        path: std::path::PathBuf,
        /// The layer read.
        layer: String,
        /// The footprints' spatial reference system, as `organization:code`.
        srs: String,
    },

    /// A file in a tile index has an empty footprint, or one without an envelope in its header.
    #[error("The footprint of {location} in the tile index {} is empty or has no envelope", path.display())]
    TileIndexFootprint {
        /// The index file.
        path: std::path::PathBuf,
        /// The file's location as written in the index.
        location: String,
    },

    /// A file location in a tile index cannot be read from.
    #[error("Cannot read {location} from the tile index {}: {error}", path.display())]
    TileIndexLocation {
        /// The index file.
        path: std::path::PathBuf,
        /// The file's location as written in the index.
        location: String,
        /// Why it cannot be read.
        error: Box<dyn std::error::Error + Send + Sync>,
    },

    /// A tile index lists no files.
    #[error("The tile index {} lists no files in {layer}", path.display())]
    TileIndexEmpty {
        /// The index file.
        path: std::path::PathBuf,
        /// The layer read.
        layer: String,
    },

    /// A tile index lists more files than can be indexed in memory.
    #[error("The tile index {} lists {count} files, more than can be indexed", path.display())]
    TileIndexTooLarge {
        /// The index file.
        path: std::path::PathBuf,
        /// How many files it lists.
        count: usize,
    },
}

impl crate::Classify for CogError {
    fn kind(&self) -> crate::ErrorKind {
        crate::ErrorKind::Internal
    }
}
