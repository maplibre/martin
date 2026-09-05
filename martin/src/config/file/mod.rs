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

#[cfg(all(feature = "contour", feature = "_tiles"))]
mod contour;
#[cfg(all(feature = "contour", feature = "_tiles"))]
pub use contour::{
    ContourElevationUnits, ContourProcessConfig, ContourRangeError, ContourSettings,
    FilteredThreshold, ResolvedContour,
};

#[cfg(all(feature = "hillshade", feature = "_tiles"))]
mod hillshade;
#[cfg(all(feature = "hillshade", feature = "_tiles"))]
pub use hillshade::{
    HillshadeFormat, HillshadeProcessConfig, HillshadeRangeError, HillshadeSettings,
    ResolvedHillshade,
};

pub mod process;
#[cfg(any(feature = "postgres", feature = "_file_kinds"))]
#[cfg(all(feature = "mlt", feature = "_tiles"))]
pub use process::{
    MltConversion, MltEncoderConfig, MltProcessConfig, MvtConversion, MvtEncoderConfig,
    MvtProcessConfig,
};
pub use process::{ProcessConfig, ProcessResolveError, ResolvedProcess};

#[cfg(any(feature = "fonts", feature = "sprites", feature = "styles"))]
mod resources;
#[cfg(any(feature = "fonts", feature = "sprites", feature = "styles"))]
pub use resources::*;

#[cfg(feature = "_tiles")]
mod tiles;
#[cfg(feature = "_tiles")]
#[allow(
    unused_imports,
    reason = "mlt feature enables _tiles without any tile source sub-features"
)]
pub use tiles::*;
