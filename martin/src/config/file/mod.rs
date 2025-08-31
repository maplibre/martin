mod file_config;
pub use file_config::*;

mod main;
pub use main::*;
pub mod cors;
pub mod srv;

#[cfg(feature = "cog")]
pub mod cog;
#[cfg(feature = "fonts")]
pub mod fonts;
#[cfg(feature = "mbtiles")]
pub mod mbtiles;
#[cfg(feature = "postgres")]
pub mod pg;
#[cfg(feature = "pmtiles")]
pub mod pmtiles;
#[cfg(feature = "sprites")]
pub mod sprites;
#[cfg(feature = "styles")]
pub mod styles;
