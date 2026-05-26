#[cfg(feature = "unstable-cog")]
pub mod cog;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
#[cfg(feature = "pmtiles")]
pub mod pmtiles;
// #[cfg(feature = "postgres")]
// pub mod postgres;

#[cfg(any(feature = "mbtiles", feature = "unstable-cog", feature = "pmtiles"))]
mod fs_helpers;

#[cfg(any(feature = "mbtiles", feature = "unstable-cog", feature = "pmtiles"))]
pub use fs_helpers::{ResolvedEntry, discover_sources_by_ext, path_modified_ms, resolve_dir_entry};
