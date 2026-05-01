mod file_config;
pub use file_config::*;

mod main;
pub use main::*;
pub mod cache;
pub mod cors;
pub mod srv;

mod error;
pub use error::{ConfigFileError, ConfigFileResult};

pub mod process;
#[cfg(all(feature = "mlt", feature = "_tiles"))]
pub use process::{MltEncoderConfig, MltProcessConfig};
pub use process::{ProcessConfig, resolve_process_config};

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
