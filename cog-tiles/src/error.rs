use std::ops::Range;

use async_tiff::error::AsyncTiffError;
use std::fmt;

use async_tiff::tags::{Compression, PhotometricInterpretation, SampleFormat, Tag};

use crate::TileCoord;

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

    /// An area is empty, not finite, inverted, or outside the world.
    #[error("Invalid bounds {bounds:?}")]
    InvalidBounds {
        /// The bounds as given.
        bounds: [f64; 4],
    },

    /// A window of pixels is too large to address.
    #[error("A window of {width}x{height} pixels is too large")]
    WindowTooLarge {
        /// Width in pixels.
        width: u64,
        /// Height in pixels.
        height: u64,
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

    /// A mosaic is asked for a number of bands it cannot serve.
    #[cfg(feature = "mosaic")]
    #[error("A mosaic serves 1 (gray) or 3 (RGB) bands, but {count} were selected")]
    MosaicBands {
        /// The number of bands selected.
        count: usize,
    },

    /// The index of a mosaic cannot be searched.
    #[cfg(feature = "mosaic")]
    #[error("Unable to search the mosaic index {location}: {source}")]
    MosaicIndex {
        /// Display form of the index.
        location: String,
        /// Why the search failed.
        source: Box<dyn std::error::Error + Send + Sync>,
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

    /// Wraps an `async-tiff` error for `location`.
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
    /// A mosaic file without `GeoTIFF` tags is not square.
    #[error("it has no GeoTIFF tags and is not square ({width}x{height} px), so it cannot be placed on tile {tile}")]
    UntaggedNotSquare {
        /// Tile from the file name.
        tile: TileCoord,
        /// Image width in pixels.
        width: u32,
        /// Image height in pixels.
        height: u32,
    },
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
    #[error("image {index} has non-square internal tiles ({width}x{height})")]
    NonSquareTiles {
        /// The IFD index.
        index: usize,
        /// Tile width in pixels.
        width: u32,
        /// Tile height in pixels.
        height: u32,
    },
    /// An image uses compressed strips.
    #[error("image {index} uses {compression:?} compressed strips, only uncompressed strips are supported")]
    CompressedStrips {
        /// The IFD index.
        index: usize,
        /// The TIFF compression.
        compression: Compression,
    },
    /// `RowsPerStrip` is 0.
    #[error("image {index} has 0 rows per strip")]
    EmptyStrips {
        /// The IFD index.
        index: usize,
    },
    /// The number of strip offsets or byte counts does not match the number of strips.
    #[error(
        "image {index} needs {strips} strips of {rows_per_strip} rows, but has {offsets} StripOffsets and {byte_counts} StripByteCounts"
    )]
    StripCount {
        /// The IFD index.
        index: usize,
        /// `RowsPerStrip` value.
        rows_per_strip: u32,
        /// Expected number of strips.
        strips: usize,
        /// Number of `StripOffsets` values.
        offsets: usize,
        /// Number of `StripByteCounts` values.
        byte_counts: usize,
    },
    /// A strip is shorter than its rows.
    #[error("strip {strip} of image {index} has {byte_count} bytes, expected {needed}")]
    ShortStrip {
        /// The IFD index.
        index: usize,
        /// Strip index.
        strip: usize,
        /// `StripByteCounts` value.
        byte_count: u64,
        /// Expected byte count.
        needed: u64,
    },
    /// `Orientation` is not 1 (top-left).
    #[error("image {index} has Orientation {orientation}, only 1 (top-left) is supported")]
    Orientation {
        /// The IFD index.
        index: usize,
        /// `Orientation` value.
        orientation: u16,
    },
    /// `FillOrder` is not 1.
    #[error("image {index} has FillOrder {fill_order}, only 1 is supported")]
    FillOrder {
        /// The IFD index.
        index: usize,
        /// `FillOrder` value.
        fill_order: u16,
    },
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
    /// An overview is tiled or striped differently from the full-resolution image.
    #[error("image {index} is {overview}, but the full-resolution image is {base}")]
    OverviewChunking {
        /// The IFD index.
        index: usize,
        /// Overview chunking.
        overview: Chunking,
        /// Full-resolution image chunking.
        base: Chunking,
    },
    /// An overview has a different compression, color type or sample layout than the full-resolution image.
    #[error("image {index} has {tag:?} {overview}, but the full-resolution image has {base}")]
    OverviewLayout {
        /// The IFD index.
        index: usize,
        /// Tag that differs.
        tag: Tag,
        /// Value in the overview.
        overview: String,
        /// Value in the full-resolution image.
        base: String,
    },
    /// An image has neither tiles nor strips.
    #[error("image {index} has neither tiles nor strips")]
    Untiled {
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
    /// The CRS cannot be reprojected to Web Mercator.
    #[error(
        "its CRS is EPSG:{epsg}, but only Web Mercator and UTM zones (EPSG:326xx, 327xx, 269xx, 258xx) can be reprojected"
    )]
    UnprojectableCrs {
        /// The EPSG code.
        epsg: u16,
    },
    /// An image to be reprojected is striped.
    #[error("image {index} is stored in strips, but only tiled images can be reprojected")]
    StripedReprojection {
        /// The IFD index.
        index: usize,
    },
    /// A tile would be read from an image much more detailed than the tile, because coarser
    /// overviews are missing.
    #[error(
        "zoom {zoom} tiles need the image downsampled {factor:.1} times, but at most {max} times is read; add overviews or raise the min zoom"
    )]
    MissingOverviews {
        /// The tile's zoom.
        zoom: u8,
        /// Image pixels per tile pixel, in each direction.
        factor: f64,
        /// The most image pixels per tile pixel that are read.
        max: f64,
    },
    /// A requested band is not in the file.
    #[error("band {band} is requested, but the file has {samples} bands")]
    MissingBand {
        /// The requested band, counted from 1.
        band: usize,
        /// Samples per pixel in the file.
        samples: usize,
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

/// Whether an image is tiled or striped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Chunking {
    /// Tiled, with this tile size.
    Tiles {
        /// Tile width in pixels.
        width: u32,
        /// Tile height in pixels.
        height: u32,
    },
    /// Striped.
    Strips,
}

impl fmt::Display for Chunking {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tiles { width, height } => write!(f, "tiled in {width}x{height} px tiles"),
            Self::Strips => f.write_str("stored in strips"),
        }
    }
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
    /// A page is not half the size of the previous image, so it is not used as an overview.
    #[error(
        "image {index} is a {width}x{height} px page, but an overview page must be {half_width}x{half_height} px (half the previous image)"
    )]
    PageNotHalf {
        /// The IFD index.
        index: usize,
        /// Page width in pixels.
        width: u32,
        /// Page height in pixels.
        height: u32,
        /// Expected width, half the previous image rounded up.
        half_width: u32,
        /// Expected height, half the previous image rounded up.
        half_height: u32,
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
