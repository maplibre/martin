use std::path::PathBuf;

use martin_tile_utils::{MAX_ZOOM, TileInfo};
use sqlite_hashes::rusqlite;

use crate::{AGG_TILES_HASH, AGG_TILES_HASH_AFTER_APPLY, AGG_TILES_HASH_BEFORE_APPLY, MbtType};

#[derive(thiserror::Error, Debug)]
pub enum MbtError {
    #[error("The source and destination MBTiles files are the same: {0}")]
    SameSourceAndDestination(PathBuf),

    #[error("The diff file and source or destination MBTiles files are the same: {0}")]
    SameDiffAndSourceOrDestination(PathBuf),

    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),

    #[error(transparent)]
    RusqliteError(#[from] rusqlite::Error),

    #[error(transparent)]
    JsonSerdeError(#[from] serde_json::Error),

    #[error("MBTile filepath contains unsupported characters: {0}")]
    UnsupportedCharsInFilepath(PathBuf),

    #[error("Inconsistent tile formats detected: {0} vs {1}")]
    InconsistentMetadata(TileInfo, TileInfo),

    #[error("Invalid data format for MBTile file {0}")]
    InvalidDataFormat(String),

    #[error("Integrity check failed for MBTile file {0} for the following reasons:\n    {1:?}")]
    FailedIntegrityCheck(String, Vec<String>),

    #[error(
        "At least one tile has mismatching hash: stored value is `{1}` != computed value `{2}` in MBTile file {0}"
    )]
    IncorrectTileHash(String, String, String),

    #[error(
        "At least one tile in the tiles table/view has an invalid value: zoom_level={1}, tile_column={2}, tile_row={3} in MBTile file {0}"
    )]
    InvalidTileIndex(String, String, String, String),

    #[error(
        "Computed aggregate tiles hash {0} does not match tile data in metadata {1} for MBTile file {2}"
    )]
    AggHashMismatch(String, String, String),

    #[error(
        "Metadata value `agg_tiles_hash` is not set in MBTiles file {0}\n    Use `mbtiles validate --agg-hash update {0}` to fix this."
    )]
    AggHashValueNotFound(String),

    #[error(r#"Filename "{0}" passed to SQLite must be valid UTF-8"#)]
    InvalidFilenameType(PathBuf),

    #[error("No tiles found")]
    NoTilesFound,

    #[error(
        "The destination file {0} is not empty. Some operations like creating a diff file require the destination file to be non-existent or empty."
    )]
    NonEmptyTargetFile(PathBuf),

    #[error("The file {0} does not have the required uniqueness constraint")]
    NoUniquenessConstraint(String),

    #[error("Could not copy MBTiles file: {reason}")]
    UnsupportedCopyOperation { reason: String },

    #[error("Unexpected duplicate tiles found when copying")]
    DuplicateValues,

    #[error("Applying a patch while diffing is not supported")]
    CannotApplyPatchAndDiff,

    #[error("The MBTiles file {0} has data of type {1}, but the desired type was set to {2}")]
    MismatchedTargetType(PathBuf, MbtType, MbtType),

    #[error(
        "Unless  --on-duplicate (override|ignore|abort)  is set, writing tiles to an existing non-empty MBTiles file is disabled. Either set --on-duplicate flag, or delete {0}"
    )]
    DestinationFileExists(PathBuf),

    #[error("Invalid zoom value {0}={1}, expecting an integer between 0..{MAX_ZOOM}")]
    InvalidZoomValue(&'static str, String),

    #[error(
        "A file {0} does not have an {AGG_TILES_HASH} metadata entry, probably because it was not created by this tool. Use `--force` to ignore this warning, or run this to update hash value: `mbtiles validate --agg-hash update {0}`"
    )]
    CannotDiffFileWithoutHash(String),

    #[error(
        "File {0} has {AGG_TILES_HASH_BEFORE_APPLY} or {AGG_TILES_HASH_AFTER_APPLY} metadata entry, indicating it is a patch file which should not be diffed with another file.  Use `--force` to ignore this warning."
    )]
    DiffingDiffFile(String),

    #[error(
        "A file {0} does not seem to be a patch diff file because it has no {AGG_TILES_HASH_BEFORE_APPLY} and {AGG_TILES_HASH_AFTER_APPLY} metadata entries.  These entries are automatically created when using `mbtiles diff` and `mbitiles copy --diff-with-file`.  Use `--force` to ignore this warning."
    )]
    PatchFileHasNoHashes(String),

    #[error(
        "A file {0} does not have {AGG_TILES_HASH_BEFORE_APPLY} metadata, probably because it was created by an older version of the `mbtiles` tool.  Use `--force` to ignore this warning, but ensure you are applying the patch to the right file."
    )]
    PatchFileHasNoBeforeHash(String),

    #[error(
        "The {AGG_TILES_HASH_BEFORE_APPLY}='{1}' in patch file {0} does not match {AGG_TILES_HASH}='{3}' in the file {2}"
    )]
    AggHashMismatchWithDiff(String, String, String, String),

    #[error(
        "The {AGG_TILES_HASH_AFTER_APPLY}='{1}' in patch file {0} does not match {AGG_TILES_HASH}='{3}' in the file {2} after the patch was applied"
    )]
    AggHashMismatchAfterApply(String, String, String, String),

    #[error(
        "MBTile of type {0} is not supported when using bin-diff.  The bin-diff format only works with flat and flat-with-hash MBTiles files."
    )]
    BinDiffRequiresFlatWithHash(MbtType),

    #[error(
        "Applying bindiff to tile {0} resulted in mismatching hash: expecting `{1}` != computed uncompressed value `{2}`"
    )]
    BinDiffIncorrectTileHash(String, String, String),

    #[error("Unable to generate or apply bin-diff patch")]
    BindiffError,

    #[error("BinDiff patch files can be only applied with `mbtiles copy --apply-patch` command")]
    UnsupportedPatchType,

    #[error(transparent)]
    IoError(#[from] std::io::Error),
}

pub type MbtResult<T> = Result<T, MbtError>;
