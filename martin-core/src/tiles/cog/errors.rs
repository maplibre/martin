//! Error types for `Cloud Optimized GeoTIFF` operations.

use std::path::PathBuf;

use png::EncodingError;
use tiff::TiffError;

/// Errors that can occur when working with COG files.
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum CogError {
    /// Cannot decode file as valid TIFF.
    #[error("Couldn't decode {1} as tiff file: {0}")]
    InvalidTiffFile(#[source] TiffError, PathBuf),

    /// Requested zoom level is outside the available range.
    #[error(
        "Requested zoom level {0} from file {1} is out of range. Possible zoom levels are {2} to {3}"
    )]
    ZoomOutOfRange(u8, PathBuf, u8, u8),

    /// No images found in TIFF file.
    #[error("Couldn't find any image in the tiff file: {0}")]
    NoImagesFound(PathBuf),

    /// Cannot seek to Image File Directory.
    #[error("Couldn't seek to ifd number {1} (0 based indexing) in tiff file {2}: {0}")]
    IfdSeekFailed(#[source] TiffError, usize, PathBuf),

    /// TIFF file contains too many images.
    #[error("Too many images in the tiff file: {0}")]
    TooManyImages(PathBuf),

    /// Required TIFF tags not found.
    #[error("Couldn't find tags {1:?} at ifd {2} of tiff file {3}: {0}")]
    TagsNotFound(#[source] TiffError, Vec<u16>, usize, PathBuf),

    /// Unsupported planar configuration in TIFF.
    #[error(
        "Unsupported planar configuration {2} at IFD {1} in TIFF file {0}. Only planar configuration 1 is supported."
    )]
    PlanarConfigurationNotSupported(PathBuf, usize, u16),

    /// Failed to read TIFF chunk data.
    #[error("Failed to read {1}th chunk(0 based index) at ifd {2} from tiff file {3}: {0}")]
    ReadChunkFailed(#[source] TiffError, u32, usize, PathBuf),

    /// Failed to write PNG header.
    #[error("Failed to write header of png file at {0}: {1}")]
    WritePngHeaderFailed(PathBuf, #[source] EncodingError),

    /// Failed to write PNG pixel data.
    #[error("Failed to write pixel bytes to png file at {0}: {1}")]
    WriteToPngFailed(PathBuf, #[source] EncodingError),

    /// Unsupported color type or bit depth.
    #[error("The color type {0:?} and its bit depth of the tiff file {1} is not supported yet")]
    NotSupportedColorTypeAndBitDepth(tiff::ColorType, PathBuf),

    /// Striped TIFF format not supported.
    #[error("Striped tiff file is not supported, the tiff file is {0}")]
    NotSupportedChunkType(PathBuf),

    /// Invalid coordinate transformation information.
    #[error("Coord transformation in {0} is invalid: {1}")]
    InvalidGeoInformation(PathBuf, String),

    /// Image pixels are not square.
    #[error(
        "The pixel size of the image {0} is not squared, the x_scale is {1}, the y_scale is {2}"
    )]
    NonSquaredImage(PathBuf, f64, f64),

    /// Cannot determine tile origin from TIFF tags.
    #[error(
        "Calculating the tile origin failed for {0}: the length of ModelTiepointTag should be >= 6, or the length of ModelTransformationTag should be >= 12"
    )]
    GetOriginFailed(PathBuf),

    /// Cannot determine full resolution from TIFF tags.
    #[error(
        "Get full resolution failed for {0}: either a valid ModelPixelScaleTag or ModelPixelScaleTag is required"
    )]
    GetFullResolutionFailed(PathBuf),

    /// Failed to create image buffer
    #[error("Failed to create image buffer for {0}: {1}")]
    ImageBufferCreationFailed(PathBuf, String),

    /// IO error.
    #[error("IO error {0}: {1}")]
    IoError(#[source] std::io::Error, PathBuf),
}
