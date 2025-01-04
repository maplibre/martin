use std::path::PathBuf;

use png::EncodingError;
use tiff::TiffError;

#[derive(thiserror::Error, Debug)]
pub enum CogError {
    #[error("Couldn't decode {1} as tiff file: {0}")]
    InvalidTiffFile(TiffError, PathBuf),

    #[error("Requested zoom level {0} from file {1} is out of range. Possible zoom levels are {2} to {3}")]
    ZoomOutOfRange(u8, PathBuf, u8, u8),

    #[error("Couldn't find any image in the tiff file: {0}")]
    NoImagesFound(PathBuf),

    #[error("Couldn't seek to ifd number {1} (0 based indexing) in tiff file {2}: {0}")]
    IfdSeekFailed(TiffError, usize, PathBuf),

    #[error("Too many images in the tiff file: {0}")]
    TooManyImages(PathBuf),

    #[error("Couldn't find tags {1:?} at ifd {2} of tiff file {3}: {0}")]
    TagsNotFound(TiffError, Vec<u16>, usize, PathBuf),

    #[error(
        "Unsupported planar configuration {2} at IFD {1} in TIFF file {0}. Only planar configuration 1 is supported."
    )]
    PlanarConfigurationNotSupported(PathBuf, usize, u16),

    #[error("Failed to read {1}th chunk(0 based index) at ifd {2} from tiff file {3}: {0}")]
    ReadChunkFailed(TiffError, u32, usize, PathBuf),

    #[error("Failed to write header of png file at {0}: {1}")]
    WritePngHeaderFailed(PathBuf, EncodingError),

    #[error("Failed to write pixel bytes to png file at {0}: {1}")]
    WriteToPngFailed(PathBuf, EncodingError),

    #[error("The color type {0:?} and its bit depth of the tiff file {1} is not supported yet")]
    NotSupportedColorTypeAndBitDepth(tiff::ColorType, PathBuf),

    #[error("Striped tiff file is not supported, the tiff file is {0}")]
    NotSupportedChunkType(PathBuf),
}
