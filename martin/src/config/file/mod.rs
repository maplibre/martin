mod file_config;
pub use file_config::*;

#[cfg(feature = "_tiles")]
mod source_location;
#[cfg(feature = "_tiles")]
pub use source_location::SourceLocation;

mod collect_unrecognized;
pub use collect_unrecognized::*;

mod main;
pub use main::*;
#[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
mod object_store;
#[cfg(any(feature = "pmtiles", feature = "unstable-cog"))]
pub(crate) use object_store::ObjectStoreConfig;
pub mod cache;
pub mod cors;
pub mod srv;
pub use srv::CacheControlHeader;

mod error;
pub use error::{ConfigFileError, ConfigFileResult};

#[cfg(all(feature = "processing", feature = "_tiles"))]
mod contour;
#[cfg(all(feature = "processing", feature = "_tiles"))]
pub use contour::{
    ContourElevationUnits, ContourProcessConfig, ContourRangeError, ContourSettings,
    FilteredThreshold, ResolvedContour,
};

#[cfg(all(feature = "processing", feature = "_tiles"))]
mod hillshade;
#[cfg(all(feature = "processing", feature = "_tiles"))]
pub use hillshade::{
    HillshadeFormat, HillshadeProcessConfig, HillshadeRangeError, HillshadeSettings,
    ResolvedHillshade,
};

pub mod process;

#[cfg(feature = "unstable-export")]
#[doc(hidden)]
pub mod tiling;

#[cfg(feature = "_tiles")]
mod tile_grids;
#[cfg(feature = "_tiles")]
pub use process::{
    MltConversion, MltEncoderConfig, MltProcessConfig, MvtConversion, MvtEncoderConfig,
    MvtProcessConfig,
};
pub use process::{ProcessConfig, ProcessResolveError, ResolvedProcess};
#[cfg(feature = "_tiles")]
pub use tile_grids::{TileGridConfig, TileGrids, TileGridsConfig};

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
