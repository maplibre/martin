use clap::ValueEnum;
use enum_display::EnumDisplay;
use serde::{Deserialize, Serialize};

#[derive(
    PartialEq, Eq, Default, Debug, Clone, Copy, Serialize, Deserialize, ValueEnum, EnumDisplay,
)]
#[cfg_attr(feature = "unstable-schemas", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
#[enum_display(case = "Kebab")]
pub enum OrderByType {
    /// Order features based on their id ascending
    #[default]
    IdAsc,
    /// Order features based on their id descending
    IdDesc,
}
