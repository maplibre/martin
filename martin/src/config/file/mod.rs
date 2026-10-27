mod file_config;
pub use file_config::*;

#[cfg(feature = "_tiles")]
mod source_location;
#[cfg(feature = "_tiles")]
pub use crate::config::file::source_location::SourceLocation;

mod collect_unrecognized;
pub use collect_unrecognized::*;

mod deserialize_fields;
pub use deserialize_fields::*;

mod main;
pub use main::*;
#[cfg(any(feature = "pmtiles", feature = "cog"))]
mod object_store;
#[cfg(any(feature = "pmtiles", feature = "cog"))]
pub(crate) use crate::config::file::object_store::ObjectStoreConfig;
pub mod cache;
pub mod cors;
pub mod srv;
pub use crate::config::file::srv::CacheControlHeader;

mod error;
pub use crate::config::file::error::{ConfigFileError, ConfigFileResult};

#[cfg(all(feature = "processing", feature = "_tiles"))]
mod contour;
#[cfg(all(feature = "processing", feature = "_tiles"))]
pub use crate::config::file::contour::{
    ContourElevationUnits, ContourProcessConfig, ContourRangeError, ContourSettings, FilteredThreshold, ResolvedContour,
};

#[cfg(all(feature = "processing", feature = "_tiles"))]
mod hillshade;
#[cfg(all(feature = "processing", feature = "_tiles"))]
pub use crate::config::file::hillshade::{
    HillshadeFormat, HillshadeProcessConfig, HillshadeRangeError, HillshadeSettings, ResolvedHillshade,
};

pub mod process;

#[cfg(feature = "unstable-generate")]
#[doc(hidden)]
pub mod tiling;

#[cfg(feature = "_tiles")]
mod tile_grids;
#[cfg(feature = "_tiles")]
pub use crate::config::file::process::{
    MltConversion, MltEncoderConfig, MltProcessConfig, MvtConversion, MvtEncoderConfig, MvtProcessConfig,
};
pub use crate::config::file::process::{ProcessConfig, ProcessResolveError, ResolvedProcess};
#[cfg(feature = "_tiles")]
pub use crate::config::file::tile_grids::{TileGridConfig, TileGrids, TileGridsConfig};

#[cfg(feature = "resources")]
mod resources;
#[cfg(feature = "resources")]
pub use resources::*;

#[cfg(feature = "_tiles")]
mod tiles;
#[cfg(feature = "_tiles")]
#[allow(
    unused_imports,
    reason = "mlt feature enables _tiles without any tile source sub-features"
)]
pub use tiles::*;
