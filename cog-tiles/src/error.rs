use std::ops::Range;

use async_tiff::error::AsyncTiffError;
use async_tiff::tags::{Compression, PhotometricInterpretation, SampleFormat};

/// A specialized [`Result`] type for `cog-tiles` operations.
pub type CogResult<T> = Result<T, CogError>;

/// Errors that can occur while opening a COG or reading its tiles.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum CogError {
    /// The backend failed to read a byte range.
    #[error("Unable to read {location}: {source}")]
    Backend {
        /// A display form of the source, e.g. a path or URL.
        location: String,
        /// The underlying error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// A byte range outside the source was requested.
    #[error("Invalid byte range {requested:?} for {location} of size {size}")]
    InvalidRange {
        /// A display form of the source.
        location: String,
        /// The requested byte range.
        requested: Range<u64>,
        /// The size of the source.
        size: u64,
    },

    /// The TIFF structure could not be parsed or a tile could not be decoded.
    #[error("Unable to decode TIFF {location}: {source}")]
    Tiff {
        /// A display form of the source.
        location: String,
        /// The underlying error.
        #[source]
        source: AsyncTiffError,
    },

    /// The TIFF is valid, but it cannot be read as web map tiles.
    #[error("{location} cannot be read as tiles: {reason}")]
    Unsupported {
        /// A display form of the source.
        location: String,
        /// Why the file cannot be read as tiles.
        reason: UnsupportedReason,
    },

    /// A tile coordinate is outside the valid range for its zoom level.
    #[error("Invalid tile coordinate {z}/{x}/{y}")]
    InvalidTileCoord {
        /// Zoom level.
        z: u8,
        /// Column.
        x: u32,
        /// Row.
        y: u32,
    },

    /// A mosaic's file name template is invalid.
    #[cfg(feature = "mosaic")]
    #[error("Invalid mosaic file name template {template:?}: {reason}")]
    InvalidTemplate {
        /// The template as given.
        template: String,
        /// What is wrong with it.
        reason: crate::MosaicTemplateError,
    },

    /// A mosaic's file zoom is above the highest zoom a COG can have.
    #[cfg(feature = "mosaic")]
    #[error("The mosaic file zoom {file_zoom} is above the highest zoom {}", crate::MAX_ZOOM)]
    FileZoomTooHigh {
        /// The requested file zoom.
        file_zoom: u8,
    },

    /// Cannot list a mosaic's files to find one to describe the mosaic with.
    #[cfg(feature = "mosaic")]
    #[error("Unable to list mosaic files under {location}: {source}")]
    MosaicListing {
        /// Display form of the mosaic location.
        location: String,
        /// The underlying object store error.
        #[source]
        source: object_store::Error,
    },

    /// One file of a mosaic cannot be read.
    #[cfg(feature = "mosaic")]
    #[error("Unable to read mosaic file {key}: {source}")]
    MosaicFile {
        /// The object key or path of the file, relative to the mosaic's location.
        key: String,
        /// Why the file cannot be read, handed to every request that waited for the same open.
        #[source]
        source: std::sync::Arc<crate::MosaicFileError>,
    },

    /// No file under a mosaic's location matches its key template.
    #[cfg(feature = "mosaic")]
    #[error("No file matching {template} was found under {location}")]
    MosaicEmpty {
        /// Display form of the mosaic location.
        location: String,
        /// The key template the files must match.
        template: String,
    },
}

impl CogError {
    /// Wraps an error from a backend reading `location`.
    pub(crate) fn backend(location: &str, source: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::Backend {
            location: location.to_owned(),
            source: source.into(),
        }
    }

    /// Reports that `location` cannot be read as tiles.
    pub(crate) fn unsupported(location: &str, reason: UnsupportedReason) -> Self {
        Self::Unsupported {
            location: location.to_owned(),
            reason,
        }
    }

    pub(crate) fn tiff(location: &str, source: AsyncTiffError) -> Self {
        Self::Tiff {
            location: location.to_owned(),
            source,
        }
    }
}

/// Why a TIFF cannot be read as web map tiles.
#[derive(thiserror::Error, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum UnsupportedReason {
    /// The file has no `GeoKeyDirectory` tag.
    #[error("it has no GeoTIFF GeoKeyDirectory")]
    NoGeoKeys,
    /// The projected CRS is not Web Mercator.
    #[error("its CRS is EPSG:{epsg}, but only EPSG:3857 (Web Mercator) is supported")]
    Crs {
        /// The `ProjectedCRSGeoKey` value.
        epsg: u16,
    },
    /// The file has no projected CRS.
    #[error("it has no projected CRS, but only EPSG:3857 (Web Mercator) is supported")]
    NoProjectedCrs,
    /// `ModelPixelScale` or `ModelTiepoint` is too short.
    #[error("ModelPixelScale or ModelTiepoint has too few values")]
    ShortTiepoint,
    /// `ModelTransformation` is too short.
    #[error("ModelTransformation has fewer than 8 values")]
    ShortTransformation,
    /// `ModelTransformation` rotates or shears the image.
    #[error("the image is rotated or sheared (ModelTransformation)")]
    Rotated,
    /// The file has no georeferencing tags.
    #[error("it has neither ModelPixelScale + ModelTiepoint nor ModelTransformation")]
    NoGeoreference,
    /// A pixel side is zero, negative or not finite.
    #[error("pixel size {x} x {y} is not positive (flipped images are not supported)")]
    NonPositivePixelSize {
        /// Pixel width in meters.
        x: f64,
        /// Pixel height in meters.
        y: f64,
    },
    /// Pixel width and height differ.
    #[error("pixels are not square ({x} x {y} m)")]
    NonSquarePixels {
        /// Pixel width in meters.
        x: f64,
        /// Pixel height in meters.
        y: f64,
    },
    /// The image origin is not finite.
    #[error("the image origin is not a finite number")]
    NonFiniteOrigin,
    /// The file has no images.
    #[error("it contains no images")]
    NoImages,
    /// Every image was skipped.
    #[error("{}", join_skipped(.0))]
    NoUsableImages(Vec<SkippedImage>),
    /// The TIFF parser panicked.
    #[error("the TIFF parser failed on malformed metadata: {0}")]
    MalformedMetadata(String),
    /// Internal tiles are not square.
    #[error("its internal tiles are not square ({width}x{height})")]
    NonSquareTiles {
        /// Tile width in pixels.
        width: u32,
        /// Tile height in pixels.
        height: u32,
    },
    /// The image is stored in strips.
    #[error("it is stored in strips, not tiles")]
    Stripped,
    /// Samples are not 8-bit unsigned integers.
    #[error("only 8-bit unsigned samples are supported, found {bits:?} bits of {formats:?}")]
    SampleType {
        /// `BitsPerSample` per band.
        bits: Vec<u16>,
        /// `SampleFormat` per band.
        formats: Vec<SampleFormat>,
    },
    /// Bands are stored in separate planes.
    #[error("bands are stored separately (PlanarConfiguration=2)")]
    PlanarBands,
    /// No output format exists for this pixel layout.
    #[error(
        "{compression:?} compression with {samples} {photometric:?} samples is not supported{}",
        if cfg!(feature = "png") { "" } else { " (the `png` feature is disabled)" }
    )]
    PixelLayout {
        /// The TIFF compression.
        compression: Compression,
        /// Samples per pixel.
        samples: u16,
        /// The photometric interpretation.
        photometric: PhotometricInterpretation,
    },
    /// An overview has a different internal tile size.
    #[error("image {index} uses a different internal tile size")]
    OverviewTileSize {
        /// The IFD index.
        index: usize,
    },
    /// An overview is compressed or laid out differently.
    #[error("image {index} is stored differently from the full-resolution image")]
    OverviewLayout {
        /// The IFD index.
        index: usize,
    },
    /// An image has no tile grid.
    #[error("image {index} is not tiled")]
    Untiled {
        /// The IFD index.
        index: usize,
    },
    /// An image has more tiles than fit in `u32`.
    #[error("image {index} has too many tiles")]
    TooManyTiles {
        /// The IFD index.
        index: usize,
    },
    /// A tile has no byte range.
    #[error("tile {col},{row} of zoom {zoom} has no byte range")]
    MissingByteRange {
        /// Zoom level.
        zoom: u8,
        /// Column within the image.
        col: usize,
        /// Row within the image.
        row: usize,
    },
    /// A decoded tile is not 8-bit.
    #[error("decoded tile is not 8-bit")]
    DecodedNotEightBit,
    /// A decoded tile has a sample count PNG cannot hold.
    #[error("{samples} samples per pixel cannot be encoded as PNG")]
    PngSamples {
        /// Samples per pixel.
        samples: usize,
    },
    /// A decoded tile is larger than the tile size or has the wrong length.
    #[error("decoded tile is {width}x{height}x{samples}, expected at most {size}x{size}")]
    DecodedSize {
        /// Decoded width in pixels.
        width: usize,
        /// Decoded height in pixels.
        height: usize,
        /// Samples per pixel.
        samples: usize,
        /// The tile size in pixels.
        size: usize,
    },
}

/// Why an image of a COG is not served, while the rest of the file is.
#[derive(thiserror::Error, Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SkippedImage {
    /// An earlier image already provides this zoom.
    #[error("image {index} has the resolution of zoom {zoom}, like an earlier image")]
    DuplicateZoom {
        /// The IFD index.
        index: usize,
        /// The zoom both images match.
        zoom: u8,
    },
    /// The resolution matches no zoom level.
    #[error(
        "image {index} has {resolution} m pixels, which is not a WebMercatorQuad zoom level for {tile_size} px tiles"
    )]
    NoMatchingZoom {
        /// The IFD index.
        index: usize,
        /// Meters per pixel.
        resolution: f64,
        /// Tile size in pixels.
        tile_size: u32,
    },
    /// The origin is off the tile grid.
    #[error("image {index} is not aligned to the zoom {zoom} tile grid")]
    Misaligned {
        /// The IFD index.
        index: usize,
        /// The matched zoom.
        zoom: u8,
    },
    /// The image extends past the edge of the world.
    #[error("image {index} extends beyond the edge of the world at zoom {zoom}")]
    BeyondWorld {
        /// The IFD index.
        index: usize,
        /// The matched zoom.
        zoom: u8,
    },
}

/// Joins skipped-image reasons with `; `.
fn join_skipped(skipped: &[SkippedImage]) -> String {
    skipped.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")
}
