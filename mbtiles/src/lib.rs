#![doc = include_str!("../README.md")]

// Re-export sqlx
pub use sqlx;

mod copier;
pub use copier::{CopyDuplicateMode, MbtilesCopier, PatchType};

mod errors;
pub use errors::{MbtError, MbtResult};

mod mbtiles;
pub use mbtiles::{CopyType, MbtTypeCli, Mbtiles};

mod metadata;
pub use metadata::Metadata;

mod patcher;
pub use patcher::apply_patch;

mod pool;
pub use pool::MbtilesPool;

mod queries;
pub use queries::*;

mod summary;

mod update;
pub use update::UpdateZoomType;

mod bindiff;

mod validation;
pub use validation::{
    calc_agg_tiles_hash, AggHashType, IntegrityCheckType, MbtType, AGG_TILES_HASH,
    AGG_TILES_HASH_AFTER_APPLY, AGG_TILES_HASH_BEFORE_APPLY,
};

/// `MBTiles` uses a TMS (Tile Map Service) scheme for its tile coordinates (inverted along the Y axis).
/// This function converts Y value between TMS tile coordinate to an XYZ tile coordinate.
/// ```
/// use mbtiles::invert_y_value;
/// assert_eq!(invert_y_value(0, 0), 0);
/// assert_eq!(invert_y_value(1, 0), 1);
/// assert_eq!(invert_y_value(1, 1), 0);
/// assert_eq!(invert_y_value(2, 0), 3);
/// assert_eq!(invert_y_value(2, 1), 2);
/// ```
#[inline]
#[must_use]
pub fn invert_y_value(zoom: u8, y: u32) -> u32 {
    (1u32 << zoom) - 1 - y
}
