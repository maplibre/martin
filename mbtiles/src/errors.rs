use std::path::PathBuf;

use martin_tile_utils::{Encoding, Format, MAX_ZOOM, TileInfo};
use sqlite_hashes::rusqlite;

use crate::{AGG_TILES_HASH, AGG_TILES_HASH_AFTER_APPLY, AGG_TILES_HASH_BEFORE_APPLY, MbtType};

#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum MbtError {
    #[error("The source and destination MBTiles files are the same: {0}")]
    SameSourceAndDestination(PathBuf),

    #[error("The diff file and source MBTiles files are the same: {0}")]
    SameDiffAndSource(PathBuf),

    #[error("The diff file and destination MBTiles files are the same: {0}")]
    SameDiffAndDestination(PathBuf),

    #[error("The patch file and source MBTiles files are the same: {0}")]
    SamePatchAndSource(PathBuf),

    #[error("The patch file and destination MBTiles files are the same: {0}")]
    SamePatchAndDestination(PathBuf),

    #[error(transparent)]
    SqlxError(#[from] sqlx::Error),

    #[error(transparent)]
    RusqliteError(#[from] rusqlite::Error),

    #[error(transparent)]
    JsonSerdeError(#[from] serde_json::Error),

    #[error("MBTile filepath contains unsupported characters: {0}")]
    UnsupportedCharsInFilepath(PathBuf),

    #[error("Inconsistent tile formats detected: {old} vs {new}")]
    InconsistentMetadata { old: TileInfo, new: TileInfo },

    #[error("Invalid data format for MBTile file {0}")]
    InvalidDataFormat(String),

    #[error(
        "File {0} exists but does not use the tile-cache schema, refusing to modify it. Use an empty/new file or an existing cache file"
    )]
    NotACacheFile(String),

    #[error("Integrity check failed for MBTile file {0} for the following reasons:\n    {1:?}")]
    FailedIntegrityCheck(String, Vec<String>),

    #[error(
        "At least one tile has mismatching hash: stored value is `{stored}` != computed value `{computed}` in MBTile file {filepath}"
    )]
    IncorrectTileHash {
        filepath: String,
        stored: String,
        computed: String,
    },

    #[error(
        "Map table references tile id `{tile_id}` that does not exist in `{table}` in MBTile file {filepath}"
    )]
    MissingTileReference {
        filepath: String,
        tile_id: String,
        table: &'static str,
    },

    #[error(
        "At least one tile in the tiles table/view has an invalid value: zoom_level={zoom_level}, tile_column={tile_column}, tile_row={tile_row} in MBTile file {filepath}"
    )]
    InvalidTileIndex {
        filepath: String,
        zoom_level: String,
        tile_column: String,
        tile_row: String,
    },

    #[error(
        "Computed aggregate tiles hash {computed} does not match tile data in metadata {stored} for MBTile file {filepath}"
    )]
    AggHashMismatch {
        computed: String,
        stored: String,
        filepath: String,
    },

    #[error(
        "Metadata value `agg_tiles_hash` is not set in MBTiles file {0}\n    Use `mbtiles validate --agg-hash update {0}` to fix this."
    )]
    AggHashValueNotFound(String),

    #[error(
        "MBTiles file {filepath} declares an unsupported tile hash algorithm `{algorithm}` in its metadata. This build can only validate `md5` hashes."
    )]
    UnsupportedHashAlgorithm {
        algorithm: String,
        filepath: PathBuf,
    },

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

    #[error(
        "An xxh3-64 hash collision was detected while bulk-copying into cache file {0}: two different tile blobs map to the same content key. Bulk SQL copy cannot resolve collisions; copy the affected tiles via the cache API (set_cached) instead"
    )]
    CacheCopyCollision(PathBuf),

    #[error("Unexpected duplicate tiles found when copying")]
    DuplicateValues,

    #[error("Applying a patch while diffing is not supported")]
    CannotApplyPatchAndDiff,

    #[error(
        "The MBTiles file {filepath} has data of type {actual}, but the desired type was set to {desired}"
    )]
    MismatchedTargetType {
        filepath: PathBuf,
        actual: MbtType,
        desired: MbtType,
    },

    #[error(
        "Unless  --on-duplicate (override|ignore|abort)  is set, writing tiles to an existing non-empty MBTiles file is disabled. Either set --on-duplicate flag, or delete {0}"
    )]
    DestinationFileExists(PathBuf),

    #[error("Invalid zoom value {0}={1}, expecting an integer between 0..{MAX_ZOOM}")]
    InvalidZoomValue(&'static str, String),

    #[error(
        "Could not find a free cache slot for tile {z}/{x}/{y} after {probes} xxh3-64 hash collisions"
    )]
    CacheKeyExhausted { z: u8, x: u32, y: u32, probes: u32 },

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
        "The {AGG_TILES_HASH_BEFORE_APPLY}='{before_apply_hash}' in patch file {patch_file} does not match {AGG_TILES_HASH}='{agg_hash}' in the file {file}"
    )]
    AggHashMismatchWithDiff {
        patch_file: String,
        before_apply_hash: String,
        file: String,
        agg_hash: String,
    },

    #[error(
        "The {AGG_TILES_HASH_AFTER_APPLY}='{after_apply_hash}' in patch file {patch_file} does not match {AGG_TILES_HASH}='{agg_hash}' in the file {file} after the patch was applied"
    )]
    AggHashMismatchAfterApply {
        patch_file: String,
        after_apply_hash: String,
        file: String,
        agg_hash: String,
    },

    #[error(
        "MBTile of type {0} is not supported when using bin-diff.  The bin-diff format only works with flat and flat-with-hash MBTiles files."
    )]
    BinDiffRequiresFlatWithHash(MbtType),

    #[error(
        "Applying bindiff to tile {tile} resulted in mismatching hash: expecting `{expected}` != computed uncompressed value `{computed}`"
    )]
    BinDiffIncorrectTileHash {
        tile: String,
        expected: String,
        computed: String,
    },

    #[error("Unable to generate or apply bin-diff patch")]
    BindiffError,

    #[error("BinDiff patch files can be only applied with `mbtiles copy --apply-patch` command")]
    UnsupportedPatchType,

    #[error("Unsupported tile file extension: {0}")]
    UnsupportedFileExtension(PathBuf),

    #[error("Inconsistent tile formats: found {new} at {path} but earlier tiles were {old}")]
    InconsistentTileFormats {
        old: Format,
        new: Format,
        path: PathBuf,
    },

    #[error("Cannot re-encode {0:?}-compressed tile data")]
    CannotRecodeCompressedTile(Encoding),

    #[error("Unsupported pack compression target: {0:?}")]
    UnsupportedPackTarget(Encoding),

    #[error("No format specified in MBTiles metadata of {0}")]
    NoFormatInMetadata(PathBuf),

    #[error("Unknown format `{format}` in MBTiles metadata of {path}")]
    UnknownFormatInMetadata { format: String, path: PathBuf },

    #[error(transparent)]
    IoError(#[from] std::io::Error),

    #[cfg(feature = "transcode")]
    #[error("Transcoding error: {0}")]
    TranscodeError(String),
}

pub type MbtResult<T> = Result<T, MbtError>;
