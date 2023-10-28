#![allow(clippy::missing_errors_doc)]

mod errors;
pub use errors::{MbtError, MbtResult};

mod mbtiles;
pub use mbtiles::{
    calc_agg_tiles_hash, IntegrityCheckType, MbtType, MbtTypeCli, Mbtiles, Metadata,
    AGG_TILES_HASH, AGG_TILES_HASH_IN_DIFF,
};

mod pool;
pub use pool::MbtilesPool;

mod copier;
pub use copier::{CopyDuplicateMode, MbtilesCopier};

mod patcher;
pub use patcher::apply_patch;

mod queries;
pub use queries::{
    create_flat_tables, create_flat_with_hash_tables, create_metadata_table,
    create_normalized_tables, is_flat_with_hash_tables_type, is_normalized_tables_type,
};
