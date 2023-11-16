#![doc = include_str!("../README.md")]
#![allow(clippy::missing_errors_doc)]

mod copier;
pub use copier::{CopyDuplicateMode, MbtilesCopier};

mod errors;
pub use errors::{MbtError, MbtResult};

mod mbtiles;
pub use mbtiles::{MbtTypeCli, Mbtiles};

mod metadata;
pub use metadata::Metadata;

mod patcher;
pub use patcher::apply_patch;

mod pool;
pub use pool::MbtilesPool;

mod queries;
pub use queries::{
    create_flat_tables, create_flat_with_hash_tables, create_metadata_table,
    create_normalized_tables, is_flat_with_hash_tables_type, is_normalized_tables_type,
};

mod summary;

mod validation;
pub use validation::{
    calc_agg_tiles_hash, IntegrityCheckType, MbtType, AGG_TILES_HASH, AGG_TILES_HASH_IN_DIFF,
};
