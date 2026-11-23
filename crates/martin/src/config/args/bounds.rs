use std::time::Duration;

use clap::ValueEnum;
use serde::{Deserialize, Serialize};

// Must match the help string for BoundsType::Quick
pub const DEFAULT_BOUNDS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(PartialEq, Eq, Default, Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, strum::Display)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "kebab-case")]
pub enum BoundsCalcType {
    /// Compute table geometry bounds, but abort if it takes longer than 5 seconds.
    #[default]
    Quick,
    /// Compute table geometry bounds. The startup time may be significant. Make sure all GEO columns have indexes.
    Calc,
    /// Skip bounds calculation. The bounds will be set to the whole world.
    Skip,
}
